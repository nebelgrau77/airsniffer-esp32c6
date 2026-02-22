#![no_std]

pub mod ui;


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
