use arrayvec::ArrayString;
use core::fmt::Write;
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::widgets::{Block, Gauge, Paragraph, Wrap};
use ratatui::{style::*, Frame};

use crate::DisplayData;

pub fn draw_welcome(frame: &mut Frame, msg: &str) {
    let text = msg;
    let paragraph = Paragraph::new(text.yellow().not_bold()).wrap(Wrap { trim: true });
    let bordered_block = Block::bordered().cyan().bold().title("AirSniffer");
    frame.render_widget(paragraph.block(bordered_block), frame.area());
}



pub fn draw(frame: &mut Frame, display_data: DisplayData) {

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

    let gauge = match display_data.ens_data.aqi {
        1 => Gauge::default()            
            .gauge_style(Style::new().cyan().on_black())
            .ratio(1_f64)
            .label("excellent"), 
        2 => Gauge::default()            
            .gauge_style(Style::new().cyan().on_black())
            .ratio(1_f64)
            .label("good"), 
        3 => Gauge::default()            
            .gauge_style(Style::new().black().on_cyan())
            .ratio(1_f64)
            .label("moderate"), 
        4 => Gauge::default()            
            .gauge_style(Style::new().black().on_yellow())
            .ratio(1_f64)
            .label("poor"), 
        5 => Gauge::default()            
            .gauge_style(Style::new().yellow().on_black())
            .ratio(1_f64)
            .label("unhealthy"), 
        _ => Gauge::default()            
            .gauge_style(Style::new().white().on_black())
            .ratio(1_f64)
            .label("unknown"), 
    
    };    

    let bordered_block = Block::bordered()
        .border_style(Style::new().cyan())                
        .title("Air Quality");
    
    frame.render_widget(gauge.block(bordered_block), second);

    // four frames - top left        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} C", round_float(display_data.bme_data.temperature)).unwrap();

    //let paragraph = Paragraph::new(textbuffer.as_str().white())
    let paragraph = Paragraph::new(textbuffer.as_str().yellow())
        .wrap(Wrap { trim: true })
        .centered();

    let bordered_block = Block::bordered()
        .border_style(Style::new().cyan())
        //.padding(Padding::new(0, 0, third_bottom_left.height / 4, 0))
        .title("Temperature");
    
    frame.render_widget(paragraph.block(bordered_block), third_bottom_left);

    // four frames - top right        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} %", round_float(display_data.bme_data.humidity)).unwrap();

    let paragraph = Paragraph::new(textbuffer.as_str().yellow())
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().cyan())
        //.padding(Padding::new(0, 0, third_bottom_right.height / 4, 0))
        .title("Humidity");

    frame.render_widget(paragraph.block(bordered_block), third_bottom_right);

    // four frames - bottom left        

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{} hPa", round_float(display_data.bme_data.pressure)).unwrap();

    let paragraph = Paragraph::new(textbuffer.as_str().yellow())
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().cyan())
        //.padding(Padding::new(0, 0, fourth_bottom_left.height / 4, 0))
        .title("Pressure");

    frame.render_widget(paragraph.block(bordered_block), fourth_bottom_left);
    
    // four frames - bottom right

    let mut textbuffer = ArrayString::<16>::new();
    write!(&mut textbuffer, "{}", display_data.ens_data.tvoc).unwrap();

    let paragraph = Paragraph::new(textbuffer.as_str().yellow())
        .wrap(Wrap { trim: true })
        .centered()
        ;

    let bordered_block = Block::bordered()
        .border_style(Style::new().cyan())
        //.padding(Padding::new(0, 0, fourth_bottom_right.height / 4, 0))
        .title("TVOC");

    frame.render_widget(paragraph.block(bordered_block), fourth_bottom_right);

}

fn round_float(val: f32) -> f32 {
    (((val * 10_f32) as i32) as f32) / 10_f32     
}
