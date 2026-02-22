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
        NoopRawMutex,                              
    }, 
    mutex::Mutex
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

// For ratatui/mousefood
use mousefood::{ColorTheme, EmbeddedBackend, EmbeddedBackendConfig, fonts, prelude::Rgb888};
use ratatui::Terminal;

// sensors
use ens160::{Ens160};
use bme280_rs::{AsyncBme280, Configuration, Oversampling, SensorMode};

// structs and drawing functions
use c6_tft::{AQIData, DisplayData, Enviro, AQISIGNAL, TRIGGER, ENVIRO_STATE, BME280_CELL};
use c6_tft::ui::{draw_dashboard, draw_welcome};
use c6_tft::tasks::{get_aqi, get_measurements};


// SPI buffer for the display
static SPI_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();

// I2C shared bus
static I2CBUS: StaticCell<Mutex<NoopRawMutex, I2c<'static, Async>>> = StaticCell::new();



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
 
    let mut led = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());

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
  
    // set up the I2C sensors
    let sensors = async {
        let i2c_bus = I2c::new(
            peripherals.I2C0,
            I2cConfig::default().with_frequency(Rate::from_khz(100)),
            )
            .map_err(|_| "could not set I2C bus")?
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
        
        ens160_aqi.reset().await.map_err(|_| "could not reset ens160")?;
        info!("ENS160 reset");
        Timer::after(Duration::from_millis(10)).await;
    
        ens160_aqi.operational().await.map_err(|_| "could not turn on ens160")?;

        let ens160id = ens160_aqi.part_id().await.map_err(|_| "could not get ens160 id")?;

        info!("ens 160 id: {}", ens160id);

        let delayns = DelayNs {};

        let bme280 = AsyncBme280::new(I2cDevice::new(bus), delayns);

        let bme280 = BME280_CELL.init(bme280);

        bme280.init().await.map_err(|_| "could not initialize bme280")?;

        bme280.set_sampling_configuration(
        Configuration::default()
            .with_temperature_oversampling(Oversampling::Oversample1)
            .with_pressure_oversampling(Oversampling::Oversample1)
            .with_humidity_oversampling(Oversampling::Oversample1)
            .with_sensor_mode(SensorMode::Normal)
        ).await.map_err(|_| "could not configure bme280")?;

        info!("BME280 set up");

        let bme280id = bme280.chip_id().await.map_err(|_| "could not get bme280 id")?;

        info!("bme280 id: {}", bme280id);

        Timer::after(Duration::from_millis(10)).await;

        let measurements = bme280.read_sample().await.map_err(|_| "could not read bme280 measurements")?;
        
        info!("calibrating...");

        led.toggle();
        ens160_aqi.set_temp((measurements.temperature.unwrap_or(25.0) * 100.0) as i16).await.map_err(|_| "could not calibrate ens160 temperature")?;
        ens160_aqi.set_hum((measurements.humidity.unwrap_or(50.0) * 100.0) as u16).await.map_err(|_| "could not calibrate ens160 humidity")?;
        Timer::after(Duration::from_millis(50)).await;
        led.toggle();

        Ok::<_, &'static str>((ens160_aqi, bme280, measurements))

    }.await;


    
    let (ens160_aqi, bme280, measurements) = match sensors {
        Ok(s) => {
            terminal.draw(
            |frame| {
                draw_welcome(frame, "sensors ready!");
            }).unwrap_or_else(|_| defmt::panic!("could not display messages"));   
            s
        }
        Err(msg) => {
            error!("sensor init failed: {}", msg);
            terminal.draw(
            |frame| {
                draw_welcome(frame, "sensors unavailable!");
            }).unwrap_or_else(|_| defmt::panic!("could not display messages"));   
            loop {}
        }

    };
        
    Timer::after(Duration::from_secs(2)).await;

    let mut last_data = DisplayData {
        bme_data: Enviro { 
            temperature: measurements.temperature.unwrap_or(0.0), 
            humidity: measurements.humidity.unwrap_or(0.0), 
            pressure: measurements.pressure.unwrap_or(0.0) },
        ens_data: AQIData { tvoc: 0, aqi: 0 }
    };

    // spawn tasks to read ENS160 and BME280 sensors
    spawner.spawn(get_aqi(ens160_aqi, led, 60u32, 5u64)).unwrap_or_else(|_| defmt::panic!("could not spawn aqi task"));
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
    
}


