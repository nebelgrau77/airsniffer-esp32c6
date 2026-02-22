use defmt::info;
use embassy_time::{Duration, Timer, Delay as DelayNs};
use esp_hal::{
    i2c::master::I2c,        
    gpio::Output,
    Async
};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use bme280_rs::AsyncBme280;
use ens160::Ens160;
use core::sync::atomic::Ordering;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;

use crate::{AQIData, SharedI2cDevice, AQISIGNAL, TRIGGER, COUNTER, ENVIRO_STATE};



#[embassy_executor::task]
pub async fn get_aqi(mut sensor: Ens160<SharedI2cDevice>, calibration: u32, freq_secs: u64) {    
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
pub async fn get_measurements(bme: &'static mut AsyncBme280<I2cDevice<'static, NoopRawMutex, I2c<'static, Async>>, DelayNs>, freq_secs: u64) {
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
pub async fn blink(mut led: Output<'static>, ms: u16) {  
    loop {
        led.toggle();
        Timer::after(Duration::from_millis(ms as u64)).await;
    }
}
