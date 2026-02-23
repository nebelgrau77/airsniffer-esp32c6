### AirSniffer :)

Get information about your indoor climate at a glance!

![Game of Life](airsniffer.gif)

Based on [this example](https://esp32.implrust.com/ratatui/hello-rust/using-mipidsi.html). Powered by [Ratatui](https://ratatui.rs) / [Mousefood](https://github.com/ratatui/mousefood/tree/main/mousefood) and [Embassy](https://embassy.dev). 

Reads and display on a regular basis data from BME280 (temperature, humidity, pressure) and ENS160 sensor (AQI and TVOC). Temperature and humidity are also used to calibrate the ENS160 on a regular basis for better accuracy.

ENS160 switches to idle mode after every measurement cycle. This way if the two sensors are in a common enclosure, its heating element does not affect the BME280 readings too much. 

**Please note:**  This makes the readings less reliable in the end, as according to ENS160 datasheet, _Further to “Initial Start-up” the conditioning or “Warm-up” period is the time required to achieve adequate sensor stability before measuring VOCs after idle periods or power-off. Typically, the ENS160 requires 3 minutes of warm-up until reasonable air quality readings can be expected_, so the proper way would be to make the sensors more distant, in a bigger enclosure.

#### Notes:

- this code uses a slightly customized version of mipidsi, with the ST7735s display set to 128x160 instead of the original 132x162; the bigger size causes distortion at one of the borders of the screen
- can be ported to ESP32S3 but not to C3 due to incompatibility of some ratatui dependencies
