#![no_std]

pub mod ui;
pub mod tasks;

use embassy_sync::{
    blocking_mutex::raw::{
        CriticalSectionRawMutex,
        NoopRawMutex},
    signal::Signal,
    mutex::Mutex,
    };
use embassy_time::Delay as DelayNs;     
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use bme280_rs::AsyncBme280;
use core::sync::atomic::AtomicU32;
use esp_hal::{i2c::master::I2c, Async};
use static_cell::StaticCell;

// ENS160 data
#[derive(Clone, Copy)]
pub struct AQIData {    
    pub tvoc: u16,    
    pub aqi: u8
    }


// BME280 data
#[derive(Clone, Copy)]

pub struct Enviro {
    pub temperature: f32,
    pub humidity: f32,
    pub pressure: f32,   
    }

// display data
#[derive(Clone, Copy)]
pub struct DisplayData {
    pub bme_data: Enviro, 
    pub ens_data: AQIData
    }


// Types defined for I2C devices (bus)
pub type SharedI2cDevice = I2cDevice<'static, NoopRawMutex, I2c<'static, Async>>;


// signals to pass between tasks
pub static AQISIGNAL: Signal<CriticalSectionRawMutex, AQIData> = Signal::new();

// shared last BME data - readable by any task, never consumed
pub static ENVIRO_STATE: Mutex<CriticalSectionRawMutex, Enviro> = Mutex::new(Enviro {
    temperature: 22.0, humidity: 50.0, pressure: 985.0
});

// wake up signal for the main loop to fire when BME data is updated
pub static TRIGGER: Signal<CriticalSectionRawMutex, ()>  = Signal::new();

// cell to wrap BME280 sensor 
pub static BME280_CELL: StaticCell<AsyncBme280<SharedI2cDevice, DelayNs>> = StaticCell::new();

// counter for calibration
pub static COUNTER: AtomicU32 = AtomicU32::new(0);
