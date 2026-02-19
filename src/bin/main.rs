#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use esp_println as _;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer, Delay as DelayNs};

use esp_hal::{
    clock::CpuClock, 
    timer::timg::TimerGroup,
    delay::Delay,
    spi::{
        master::{
        Config as SpiConfig,
        Spi
        },
        Mode as SpiMode,
    },
    i2c::master::
        {I2c, Config as I2cConfig
        },
    time::Rate,
    gpio::{
        Level,
        Output,
        OutputConfig
    },  
    Async
};

use embassy_sync::{
    signal::Signal,
    blocking_mutex::raw::{
        CriticalSectionRawMutex,
        NoopRawMutex,                              
    },
    mutex::Mutex,    
    pubsub::PubSubChannel,    
};

use static_cell::StaticCell;
use embedded_hal_bus::spi::ExclusiveDevice;
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;

// TFT Screen stuff
use mipidsi::{
    Builder, 
    models::{        
        ST7735s},
    interface::SpiInterface, 
    options::{
        Orientation, Rotation
    }
};

// Embedded graphics stuff
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::text::{Baseline, Text};

// Larger font
use profont::{PROFONT_10_POINT};

use core::fmt::Write;   
use arrayvec::ArrayString;

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

// For ratatui
use mousefood::{EmbeddedBackend, EmbeddedBackendConfig, fonts};
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::widgets::{Block, Paragraph, Wrap, Gauge};
use ratatui::{style::*, Frame, Terminal};

// sensors
use ens160::{Ens160};
use bme280_rs::{AsyncBme280, Configuration, Oversampling, SensorMode};

// Types defined for I2C devices (bus)
type SharedI2cDevice = I2cDevice<'static, NoopRawMutex, I2c<'static, Async>>;

// SPI buffer for the display
static SPI_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();

// I2C shared bus
static I2CBUS: StaticCell<Mutex<NoopRawMutex, I2c<'static, Async>>> = StaticCell::new();

// signals to pass between tasks
static AQISIGNAL: Signal<CriticalSectionRawMutex, AirQualityData> = Signal::new();

// Signal to pass BME data between tasks
static ENVIROSIGNAL: PubSubChannel<CriticalSectionRawMutex, Enviro, 1,3, 1> = PubSubChannel::new();

// BME280 sensor
static BME280_CELL: StaticCell<AsyncBme280<SharedI2cDevice, DelayNs>> = StaticCell::new();

// counter for calibration
static COUNTER: AtomicU32 = AtomicU32::new(0);

// structs to hold sensor data
#[derive(Clone, Copy)]
struct AirQualityData {
    temperature: f32,    
    humidity: f32,   
    pressure: f32,
    tvoc: u16,    
    aqi: u8
    }

#[derive(Clone, Copy)]
struct Enviro {
    temperature: f32,
    humidity: f32,
    pressure: f32,   
    }


#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

extern crate alloc;


// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.2.0

    //rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    info!("Embassy initialized!");
    
    let i2c_bus = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(100)),
    )
    .unwrap()
    .with_scl(peripherals.GPIO23)
    .with_sda(peripherals.GPIO22)
    .into_async();

    info!("I2C initialized!");

    let bus = Mutex::<NoopRawMutex, _>::new(i2c_bus);
    let bus = I2CBUS.init(bus);

    info!("shared I2C bus set up");

    let mut ens160_aqi = Ens160::new(I2cDevice::new(bus), 0x53);
    info!("Initialized ENS160");

    Timer::after(Duration::from_millis(10)).await;
    
    ens160_aqi.reset().await.ok();
    info!("ENS160 reset");
    Timer::after(Duration::from_millis(10)).await;
    
    ens160_aqi.operational().await.ok();

    info!("ens 160 id: {}", ens160_aqi.part_id().await.unwrap());

    let delayns = DelayNs {};

    let bme280 = AsyncBme280::new(I2cDevice::new(bus), delayns);

    let bme280 = BME280_CELL.init(bme280);

    bme280.init().await.unwrap();

    bme280.set_sampling_configuration(
    Configuration::default()
        .with_temperature_oversampling(Oversampling::Oversample1)
        .with_pressure_oversampling(Oversampling::Oversample1)
        .with_humidity_oversampling(Oversampling::Oversample1)
        .with_sensor_mode(SensorMode::Normal)
    ).await.unwrap();

    info!("BME280 set up");
    
    info!("bme280 id: {}", bme280.chip_id().await.unwrap());

    Timer::after(Duration::from_millis(10)).await;

    if let Some(temperature) = bme280.read_temperature().await.unwrap() {
    info!("Temperature: {} C", temperature);
} else {
    info!("Temperature reading was disabled");
}
 

    // set up display

    let spi = Spi::new(
    peripherals.SPI2,
    SpiConfig::default()
        .with_frequency(Rate::from_mhz(25))
        .with_mode(SpiMode::_0))
        .unwrap()
        .with_sck(peripherals.GPIO19)
        .with_mosi(peripherals.GPIO18);

    let cs = Output::new(peripherals.GPIO1, Level::Low, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());

    let reset = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let buffer = SPI_BUFFER.init([0; 1024]);

    let spi_dev = ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let interface = SpiInterface::new(spi_dev, dc, buffer);
 
    let mut display = Builder::new(
    ST7735s,
    interface
    )
    .reset_pin(reset)
    .init(&mut Delay::new())
    .unwrap();

    // CRITICAL: Set orientation BEFORE clearing and creating backend
    display.set_orientation(
    Orientation::default().rotate(Rotation::Deg90)
    ).unwrap();

    // Clear with the new orientation
    display.clear(Rgb565::BLACK).unwrap();


    let text_style = MonoTextStyle::new(&PROFONT_10_POINT, Rgb565::CSS_GREEN_YELLOW);
    Text::with_baseline("system starting...", Point::new(10, 10), text_style, Baseline::Top)
    .draw(&mut display)
    .unwrap();
    
    Timer::after(Duration::from_secs(2)).await;

    
    display.clear(Rgb565::BLACK).unwrap();

    Timer::after(Duration::from_millis(500)).await;
    
    let led = Output::new(peripherals.GPIO15, Level::Low, OutputConfig::default());

    // TODO: Spawn some tasks
    spawner.spawn(get_aqi(ens160_aqi, 5u32)).ok();
    spawner.spawn(get_measurements(bme280)).ok();
    //spawner.spawn(blink(led, 1000)).ok();
    
    // Create a custom config with a flush callback
    let backend_config = EmbeddedBackendConfig 
    {
        font_regular: fonts::MONO_6X12_OPTIMIZED,        
        ..Default::default()        
    };


    let backend = EmbeddedBackend::new(&mut display, backend_config);
    let mut terminal = Terminal::new(backend).unwrap();

    info!("mousefood set up");




    loop {
    
        let aqidata = AQISIGNAL.wait().await;
        info!("got data: temp {}, hum {}, press {}, tvoc {}, aqi {}", aqidata.temperature, aqidata.humidity, aqidata.pressure, aqidata.tvoc, aqidata.aqi);
        
        terminal.draw(
        |frame| {
                draw(frame, aqidata);
        }).unwrap();    
  
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0/examples
}

const CINFO: Color = Color::Rgb(76, 209, 224);
const CWARNING: Color = Color::Rgb(209, 154, 102);
//const BKGD: Color = Color::Rgb(35, 39, 46);


fn draw(frame: &mut Frame, aqidata: AirQualityData) {

    let vertical = Layout::vertical([
        //Constraint::Percentage(30),         
        Constraint::Percentage(30), 
        Constraint::Percentage(30),
        Constraint::Percentage(30), 
        ]).flex(Flex::Center);
    let [//first,         
        second, 
         third ,
         fourth,         
        ] = vertical.areas(frame.area());

    let horizontal_third = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]);
    let [third_bottom_left, third_bottom_right] = horizontal_third.areas(third);
    let horizontal_fourth = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]);
    let [fourth_bottom_left, fourth_bottom_right] = horizontal_fourth.areas(fourth);
    
    
    info!("AQI: {}", aqidata.aqi);

    let gauge = match aqidata.aqi {
        1 => Gauge::default()            
            .gauge_style(Style::new().fg(CINFO).bg(Color::Black))
            .ratio(1_f64)
            .label("excellent"), 
        2 => Gauge::default()            
            .gauge_style(Style::new().fg(CINFO).bg(Color::Black))
            .ratio(1_f64)
            .label("good"), 
        3 => Gauge::default()            
            .gauge_style(Style::new().fg(Color::Black).bg(CINFO))
            .ratio(1_f64)
            .label("moderate"), 
        4 => Gauge::default()            
            .gauge_style(Style::new().fg(Color::Black).bg(CWARNING))
            .ratio(1_f64)
            .label("poor"), 
        5 => Gauge::default()            
            .gauge_style(Style::new().fg(CWARNING).bg(Color::Black))
            .ratio(1_f64)
            .label("unhealthy"), 
        _ => Gauge::default()            
            .gauge_style(Style::new().fg(Color::White).bg(Color::Black))
            .ratio(1_f64)
            .label("unknown"), 
    
    };    

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(CINFO))                
        .title("Air Quality");
    
    frame.render_widget(gauge.block(bordered_block), second);

    // four frames - top left        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} C", round_float(aqidata.temperature)).unwrap();

    //let paragraph = Paragraph::new(textbuffer.as_str().white())
    let paragraph = Paragraph::new(textbuffer.as_str().fg(CWARNING))
        .wrap(Wrap { trim: true })
        .centered();

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(CINFO))
        //.padding(Padding::new(0, 0, third_bottom_left.height / 4, 0))
        .title("Temperature");
    
    frame.render_widget(paragraph.block(bordered_block), third_bottom_left);

    // four frames - top right        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} %", round_float(aqidata.humidity)).unwrap();

    let paragraph = Paragraph::new(textbuffer.as_str().fg(CWARNING))
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(CINFO))
        //.padding(Padding::new(0, 0, third_bottom_right.height / 4, 0))
        .title("Humidity");

    frame.render_widget(paragraph.block(bordered_block), third_bottom_right);


    // four frames - bottom left        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} hPa", round_float(aqidata.pressure)).unwrap();


    let paragraph = Paragraph::new(textbuffer.as_str().fg(CWARNING))
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(CINFO))
        //.padding(Padding::new(0, 0, fourth_bottom_left.height / 4, 0))
        .title("Pressure");

    frame.render_widget(paragraph.block(bordered_block), fourth_bottom_left);

    
    // four frames - bottom right

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{}", aqidata.tvoc).unwrap();

    let paragraph = Paragraph::new(textbuffer.as_str().fg(CWARNING))
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(CINFO))
        //.padding(Padding::new(0, 0, fourth_bottom_right.height / 4, 0))
        .title("TVOC");

    frame.render_widget(paragraph.block(bordered_block), fourth_bottom_right);

}


#[embassy_executor::task]
async fn get_aqi(mut sensor: Ens160<SharedI2cDevice>, calibration: u32) {    
    // check if ENS160 is ready and get data
    // get BME280 data
    // pass it all to Signal

    let mut sub_bme = ENVIROSIGNAL.subscriber().unwrap();

    loop {    
        if let Ok(status) = sensor.status().await {
            if status.data_is_ready() {                                    
                let envi = sub_bme.next_message_pure().await;
                let airquality = AirQualityData {
                    tvoc: sensor.tvoc().await.unwrap(),                    
                    temperature: envi.temperature,
                    humidity: envi.humidity,
                    pressure: envi.pressure,
                    aqi: sensor.air_quality_index().await.unwrap() as u8
                };
                AQISIGNAL.signal(airquality);
                info!("got air quality data from sensor");
                let counter = COUNTER.load(core::sync::atomic::Ordering::Relaxed);
                if counter >= calibration {
                    info!("time to calibrate...");
                    COUNTER.store(0, Ordering::Relaxed);
                } else {
                    COUNTER.store(counter.wrapping_add(1), Ordering::Relaxed);
                }
            }
        Timer::after(Duration::from_secs(2)).await;
        }
    }
}


#[embassy_executor::task]
// blinkenlight
async fn blink(mut led: Output<'static>, ms: u16) {  
    loop {
        led.toggle();
        Timer::after(Duration::from_millis(ms as u64)).await;
    }
}


#[embassy_executor::task]
async fn get_measurements(bme: &'static mut AsyncBme280<I2cDevice<'static, NoopRawMutex, I2c<'static, Async>>, DelayNs>) {
    // get temperature, humidity and pressure from BME280 sensor and publish as ENVIROSIGNAL
    let pub_bme = ENVIROSIGNAL.publisher().unwrap();
   
    loop {

        let measurements = bme.read_sample().await.unwrap();

        
        /*
        info!("task - Got BME measurements! T: {}°C, RH: {}%, P: {} Pa",             
            measurements.temperature,
            measurements.humidity,
            measurements.pressure
        );
         */

        let envdata = Enviro {
            temperature: measurements.temperature.unwrap_or(0.0),
            humidity: measurements.humidity.unwrap_or(0.0),
            pressure: measurements.pressure.unwrap_or(0.0) / 100.0,                   
        };

        pub_bme.publish_immediate(envdata);
        
        Timer::after(Duration::from_secs(1)).await;
    }
}


fn round_float(val: f32) -> f32 {
    (((val * 10_f32) as i32) as f32) / 10_f32     
}
