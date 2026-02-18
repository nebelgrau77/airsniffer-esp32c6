#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_graphics::mock_display::ColorMapping;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_println as _;

use esp_hal::{
  delay::Delay,
  spi::{
    master::{
      Config as SpiConfig,
      Spi
    },
    Mode as SpiMode,
  },
  time::Rate,
  gpio::{
    Level,
    Output,
    OutputConfig
  },  
};



use esp_hal::i2c::master::{I2c, Config as I2cConfig};

use core::fmt::Write;   
use arrayvec::ArrayString;

use static_cell::StaticCell;
use embedded_hal_bus::spi::ExclusiveDevice;


// Embedded graphics stuff
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

// Larger font
use profont::{PROFONT_10_POINT, PROFONT_14_POINT, PROFONT_18_POINT, PROFONT_24_POINT};

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

// For ratatui
use mousefood::{EmbeddedBackend, EmbeddedBackendConfig, fonts};
use ratatui::{layout::{Constraint, Flex, Layout}};
use ratatui::widgets::{Block, Paragraph, Wrap, Gauge};
use ratatui::{style::*, Frame, Terminal};


#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

extern crate alloc;

// Embedded Grpahics related
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};

static SPI_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();

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


    /*
    let i2c_bus = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(100)),
    )
    .unwrap()
    .with_scl(peripherals.GPIO23)
    .with_sda(peripherals.GPIO22);

    info!("I2C initialized!");

 */

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


    const TEAL: Rgb565 = Rgb565::new(100, 220, 220); //Color = Color::Rgb(100, 220, 220);
    const ORANGE: Rgb565 = Rgb565::new(250, 145, 55);


    let text_style = MonoTextStyle::new(&PROFONT_24_POINT, TEAL);
    Text::with_baseline("test", Point::new(30, 10), text_style, Baseline::Top)
    .draw(&mut display)
    .unwrap();

    let text_style = MonoTextStyle::new(&PROFONT_14_POINT, ORANGE);

    Text::with_baseline("of TFT", Point::new(20, 50), text_style, Baseline::Top)
        .draw(&mut display)
        .unwrap();

    Timer::after(Duration::from_secs(5)).await;

    display.clear(Rgb565::WHITE).unwrap();

    Timer::after(Duration::from_millis(500)).await;

    display.clear(Rgb565::RED).unwrap();

    Timer::after(Duration::from_millis(500)).await;

    display.clear(Rgb565::BLACK).unwrap();

    Timer::after(Duration::from_secs(2)).await;

    // TODO: Spawn some tasks
    let _ = spawner;

    
    // Create a custom config with a flush callback
    let backend_config = EmbeddedBackendConfig 
    {
        font_regular: fonts::MONO_6X12_OPTIMIZED,
        
        ..Default::default()        
    };


    let backend = EmbeddedBackend::new(&mut display, backend_config);

    let mut terminal = Terminal::new(backend).unwrap();

      info!("mousefood set up");

    //terminal.draw(draw).unwrap();

    /*
    terminal.draw(
        |frame| {
                draw(frame);
        }).unwrap();    
    */

    let mut val: u16 = 0;

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
        
        terminal.draw(
        |frame| {
                draw(frame, val);
        }).unwrap();    

        val += 1;
    
  
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0/examples
}




 const TEAL: Color = Color::Rgb(100, 220, 220);
const ORANGE: Color = Color::Rgb(250, 145, 55);

fn draw(frame: &mut Frame, value: u16) {
        
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
    
    
    let gauge = Gauge::default()            
            .gauge_style(Style::new().fg(TEAL).bg(Color::Black))
            .ratio(1_f64)
            .label("excellent");
        

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(TEAL))                
        .title("Air Quality");
    
    frame.render_widget(gauge.block(bordered_block), second);
     

    // four frames - top left        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} C", round_float(23.5)).unwrap();

    //let paragraph = Paragraph::new(textbuffer.as_str().white())
    let paragraph = Paragraph::new(textbuffer.as_str().fg(ORANGE))
        .wrap(Wrap { trim: true })
        .centered();

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(TEAL))
        //.padding(Padding::new(0, 0, third_bottom_left.height / 4, 0))
        .title("Temperature");
    
    frame.render_widget(paragraph.block(bordered_block), third_bottom_left);

    // four frames - top right        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} %", round_float(65.3)).unwrap();

    let paragraph = Paragraph::new(textbuffer.as_str().fg(ORANGE))
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(TEAL))
        //.padding(Padding::new(0, 0, third_bottom_right.height / 4, 0))
        .title("Humidity");

    frame.render_widget(paragraph.block(bordered_block), third_bottom_right);


    // four frames - bottom left        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} hPa", round_float(985.2)).unwrap();


    let paragraph = Paragraph::new(textbuffer.as_str().fg(ORANGE))
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(TEAL))
        //.padding(Padding::new(0, 0, fourth_bottom_left.height / 4, 0))
        .title("Pressure");

    frame.render_widget(paragraph.block(bordered_block), fourth_bottom_left);

    
    // four frames - bottom right

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{}", value).unwrap();

    

    let paragraph = Paragraph::new(textbuffer.as_str().fg(ORANGE))
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().fg(TEAL))
        //.padding(Padding::new(0, 0, fourth_bottom_right.height / 4, 0))
        .title("TVOC");

    frame.render_widget(paragraph.block(bordered_block), fourth_bottom_right);

}


fn round_float(val: f32) -> f32 {
    (((val * 100_f32) as i32) as f32) / 100_f32     
}