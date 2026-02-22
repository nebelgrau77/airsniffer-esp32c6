#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::{info, error};
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
    blocking_mutex::raw::{
        CriticalSectionRawMutex,
        NoopRawMutex,                              
    }, mutex::Mutex, signal::Signal    
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

// Embedded graphics components
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

// For ratatui/mousefood
use mousefood::{ColorTheme, EmbeddedBackend, EmbeddedBackendConfig, fonts, prelude::Rgb888};
use ratatui::Terminal;

// sensors
use ens160::{Ens160};
use bme280_rs::{AsyncBme280, Configuration, Oversampling, SensorMode};

// structs and drawing functions
use c6_tft::{AQIData, DisplayData, Enviro};
use c6_tft::ui::{draw_dashboard, draw_welcome};

// Types defined for I2C devices (bus)
type SharedI2cDevice = I2cDevice<'static, NoopRawMutex, I2c<'static, Async>>;

// SPI buffer for the display
static SPI_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();

// I2C shared bus
static I2CBUS: StaticCell<Mutex<NoopRawMutex, I2c<'static, Async>>> = StaticCell::new();

// signals to pass between tasks
static AQISIGNAL: Signal<CriticalSectionRawMutex, AQIData> = Signal::new();

// shared last BME data - readable by any task, never consumed
static ENVIRO_STATE: Mutex<CriticalSectionRawMutex, Enviro> = Mutex::new(Enviro {
    temperature: 22.0, humidity: 50.0, pressure: 985.0
});

// wake up signal for the main loop to fire when BME data is updated
static TRIGGER: Signal<CriticalSectionRawMutex, ()>  = Signal::new();

// cell to wrap BME280 sensor 
static BME280_CELL: StaticCell<AsyncBme280<SharedI2cDevice, DelayNs>> = StaticCell::new();

// counter for calibration
static COUNTER: AtomicU32 = AtomicU32::new(0);


#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    error!("panic: {}", defmt::Display2Format(info));
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

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    info!("Embassy initialized!");

    // set up display

    let spi = Spi::new(
    peripherals.SPI2,
    SpiConfig::default()
        .with_frequency(Rate::from_mhz(25))
        .with_mode(SpiMode::_0))
        .unwrap_or_else(|_| defmt::panic!("could not set SPI"))
        .with_sck(peripherals.GPIO19)
        .with_mosi(peripherals.GPIO18);

    let cs = Output::new(peripherals.GPIO1, Level::Low, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());

    let reset = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let buffer = SPI_BUFFER.init([0; 1024]);

    let spi_dev = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|_| defmt::panic!("could not set SPI device"));
    let interface = SpiInterface::new(spi_dev, dc, buffer);
 
    let mut display = Builder::new(
    ST7735s,
    interface
    )
    .reset_pin(reset)
    .init(&mut Delay::new())
    .unwrap_or_else(|_| defmt::panic!("could not set display"));

    // CRITICAL: Set orientation BEFORE clearing and creating backend
    display.set_orientation(
    Orientation::default().rotate(Rotation::Deg90)
    )  .unwrap_or_else(|_| defmt::panic!("could not set display orientation"));
    
    display.clear(Rgb565::BLACK).unwrap_or_else(|_| defmt::panic!("could not clear display"));

    info!("display set up");

    Timer::after(Duration::from_millis(500)).await;
 
    //let led = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());

    let theme = ColorTheme {          
        yellow: Rgb888::new(255,100,0),
        ..ColorTheme::ansi()
    };
  
    // Create a custom config for the mousefood terminal
    let backend_config = EmbeddedBackendConfig 
    {
        font_regular: fonts::MONO_6X12_OPTIMIZED,   
        font_bold: Some(fonts::MONO_7X13),    
        color_theme: theme,        
        ..Default::default()        
    };

    let backend = EmbeddedBackend::new(&mut display, backend_config);
    let mut terminal = Terminal::new(backend).unwrap_or_else(|_| defmt::panic!("could not set mousefood terminal"));

    info!("mousefood set up");

    for msg in ["powered by Ratatui/Mousefood     and Embassy", "system starting..."] {

        terminal.draw(
        |frame| {
                draw_welcome(frame, msg);
        }).unwrap_or_else(|_| defmt::panic!("could not display message"));   

    Timer::after(Duration::from_secs(2)).await;

    }
  
    let i2c_bus = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(100)),
    )
    .unwrap_or_else(|_| defmt::panic!("could not set I2C bus"))
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
    
    ens160_aqi.reset().await.unwrap_or_else(|_| defmt::panic!("could not reset ens160"));
    info!("ENS160 reset");
    Timer::after(Duration::from_millis(10)).await;
    
    ens160_aqi.operational().await.unwrap_or_else(|_| defmt::panic!("could not turn on ens160"));

    info!("ens 160 id: {}", ens160_aqi.part_id().await.unwrap_or_else(|_| defmt::panic!("could not get ens160 id")));

    let delayns = DelayNs {};

    let bme280 = AsyncBme280::new(I2cDevice::new(bus), delayns);

    let bme280 = BME280_CELL.init(bme280);

    bme280.init().await.unwrap_or_else(|_| defmt::panic!("could not initialize bme280"));

    bme280.set_sampling_configuration(
    Configuration::default()
        .with_temperature_oversampling(Oversampling::Oversample1)
        .with_pressure_oversampling(Oversampling::Oversample1)
        .with_humidity_oversampling(Oversampling::Oversample1)
        .with_sensor_mode(SensorMode::Normal)
    ).await.unwrap_or_else(|_| defmt::panic!("could not configure bme280"));

    info!("BME280 set up");
    
    info!("bme280 id: {}", bme280.chip_id().await.unwrap_or_else(|_| defmt::panic!("could not get bme280 id")));

    Timer::after(Duration::from_millis(10)).await;

    let measurements = bme280.read_sample().await.unwrap_or_else(|_| defmt::panic!("could not read bme280 measurements"));
    
    info!("calibrating...");

    ens160_aqi.set_temp((measurements.temperature.unwrap_or(25.0) * 100.0) as i16).await.unwrap_or_else(|_| defmt::panic!("could not calibrate ens160 temperature"));
    ens160_aqi.set_hum((measurements.humidity.unwrap_or(50.0) * 100.0) as u16).await.unwrap_or_else(|_| defmt::panic!("could not calibrate ens160 humidity"));

    terminal.draw(
        |frame| {
                draw_welcome(frame, "sensors ready!");
        }).unwrap_or_else(|_| defmt::panic!("could not display messages"));   

    Timer::after(Duration::from_secs(2)).await;

    let mut last_data = DisplayData {
        bme_data: Enviro { 
            temperature: measurements.temperature.unwrap_or(0.0), 
            humidity: measurements.humidity.unwrap_or(0.0), 
            pressure: measurements.pressure.unwrap_or(0.0) },
        ens_data: AQIData { tvoc: 0, aqi: 0 }
    };

    // TODO: Spawn some tasks
    spawner.spawn(get_aqi(ens160_aqi, 5u32, 4u64)).unwrap_or_else(|_| defmt::panic!("could not spawn aqi task"));
    spawner.spawn(get_measurements(bme280, 2u64)).unwrap_or_else(|_| defmt::panic!("could not spawn bme280 task"));
    
    loop {
        // wait for the trigger to update display with sensor data

        TRIGGER.wait().await;

        let enviro = ENVIRO_STATE.lock().await;

        info!("got data: temp {}, hum {}, press {}", enviro.temperature, enviro.humidity, enviro.pressure);

        last_data.bme_data.temperature = enviro.temperature;
        last_data.bme_data.humidity = enviro.humidity;
        last_data.bme_data.pressure = enviro.pressure;

        drop(enviro);

        if let Some(aqidata) = AQISIGNAL.try_take() {
            last_data.ens_data.tvoc = aqidata.tvoc;
            last_data.ens_data.aqi = aqidata.aqi;
        }

        terminal.draw(
        |frame| {
                draw_dashboard(frame, last_data);
        }).unwrap_or_else(|_| defmt::panic!("could not display dashboard"));    
  
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0/examples
}


#[embassy_executor::task]
async fn get_aqi(mut sensor: Ens160<SharedI2cDevice>, calibration: u32, freq_secs: u64) {    
    // check if ENS160 is ready and get its data
    // update the sensor state with the results
    // if it's time to calibrate, get BME280 data and update ENS160 reference temperature and humidity
    
    loop {    
        info!("wake up the sensor...");
        sensor.operational().await.unwrap_or_else(|_| defmt::panic!("could not make ens160 operational"));
        Timer::after(Duration::from_millis(1000)).await;

        if let Ok(status) = sensor.status().await {
            if status.data_is_ready() {                                    
                let airquality = AQIData {
                    tvoc: sensor.tvoc().await.unwrap_or_else(|_| defmt::panic!("could not get ens160 AQI")),                                        
                    aqi: sensor.air_quality_index().await.unwrap_or_else(|_| defmt::panic!("could not get ens160 TVOC")) as u8
                };
                
                AQISIGNAL.signal(airquality);

                info!("got air quality data from sensor");
                let counter = COUNTER.load(core::sync::atomic::Ordering::Relaxed);
                if counter >= calibration {
                    info!("time to calibrate...");
                    let envi = ENVIRO_STATE.lock().await;
                    info!("got data for calibration: {}°C, {} %", envi.temperature, envi.humidity);
                    sensor.set_temp((envi.temperature * 100.0) as i16).await.unwrap_or_else(|_| defmt::panic!("could not calibrate ens160 temperature"));
                    sensor.set_hum((envi.humidity * 100.0) as u16).await.unwrap_or_else(|_| defmt::panic!("could not calibrate ens160 humidity"));
                    info!("sensor calibrated");
                    drop(envi); // release lock
                    COUNTER.store(0, Ordering::Relaxed);
                } else {
                    COUNTER.store(counter.wrapping_add(1), Ordering::Relaxed);
                }
            }
        
        sensor.idle().await.unwrap_or_else(|_| defmt::panic!("could not set ens160 to idle mode"));
        info!("sensor put to sleep...");
        Timer::after(Duration::from_secs(freq_secs)).await;
        }
    }
}




#[embassy_executor::task]
async fn get_measurements(bme: &'static mut AsyncBme280<I2cDevice<'static, NoopRawMutex, I2c<'static, Async>>, DelayNs>, freq_secs: u64) {
    // get temperature, humidity and pressure from BME280 sensor
    // update the sensor state and trigger the display every few seconds 
    // (BME280 data displayed more frequently)
        
    loop {

        let measurements = bme.read_sample().await.unwrap_or_else(|_| defmt::panic!("could not read bme280 measurements"));
        
        info!("task - Got BME measurements! T: {}°C, RH: {}%, P: {} Pa",             
            measurements.temperature.unwrap_or(0.0),
            measurements.humidity.unwrap_or(0.0),
            measurements.pressure.unwrap_or(0.0)
        );
        
        {
            let mut state = ENVIRO_STATE.lock().await;
            state.temperature = measurements.temperature.unwrap_or(0.0);
            state.humidity = measurements.humidity.unwrap_or(0.0);
            state.pressure = measurements.pressure.unwrap_or(0.0) / 100.0;
        }
        
        TRIGGER.signal(());
    
        Timer::after(Duration::from_secs(freq_secs)).await;
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
