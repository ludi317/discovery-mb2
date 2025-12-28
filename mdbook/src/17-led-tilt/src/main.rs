#![deny(unsafe_code)]
#![no_main]
#![no_std]

use cortex_m_rt::entry;
use embedded_hal::delay::DelayNs;
use panic_rtt_target as _;
use rtt_target::rtt_init_print;

use microbit::{
    display::blocking::Display,
    hal::{twim, Timer},
    pac::twim0::frequency::FREQUENCY_A,
};

use lsm303agr::{AccelMode, AccelOutputDataRate, Lsm303agr};

#[entry]
fn main() -> ! {
    rtt_init_print!();
    let board = microbit::Board::take().unwrap();

    let i2c = { twim::Twim::new(board.TWIM0, board.i2c_internal.into(), FREQUENCY_A::K100) };
    let mut timer = Timer::new(board.TIMER0);
    let mut display = Display::new(board.display_pins);

    // Initialize accelerometer
    let mut sensor = Lsm303agr::new_with_i2c(i2c);
    sensor.init().unwrap();
    sensor
        .set_accel_mode_and_odr(
            &mut timer,
            AccelMode::HighResolution,
            AccelOutputDataRate::Hz50,
        )
        .unwrap();

    // LED grid (5x5)
    let mut leds = [[0u8; 5]; 5];

    // Starting position (center of the grid)
    let mut led_row: i8 = 2;
    let mut led_col: i8 = 2;

    // Tilt threshold in milliG (1000 mg = 1g)
    // Adjust this value to make it more or less sensitive
    const TILT_THRESHOLD: i32 = 250;

    loop {
        // Wait for new accelerometer data
        while !sensor.accel_status().unwrap().xyz_new_data() {
            timer.delay_ms(1u32);
        }

        // Read acceleration values
        let (x, y, _z) = sensor.acceleration().unwrap().xyz_mg();

        // Clear current LED position
        leds[led_row as usize][led_col as usize] = 0u8;

        // Update position based on tilt
        // X axis: positive = tilt right, negative = tilt left
        if x > TILT_THRESHOLD {
            led_col = (led_col + 1).min(4); // Move right, max column is 4
        } else if x < -TILT_THRESHOLD {
            led_col = (led_col - 1).max(0); // Move left, min column is 0
        }

        // Y axis: positive = tilt forward (toward top edge), negative = tilt back
        // Row 0 is at the top, row 4 is at the bottom
        if y > TILT_THRESHOLD {
            led_row = (led_row - 1).max(0); // Move up, min row is 0
        } else if y < -TILT_THRESHOLD {
            led_row = (led_row + 1).min(4); // Move down, max row is 4
        }

        // Set new LED position
        leds[led_row as usize][led_col as usize] = 255u8;

        // Display the LED grid for 100ms
        display.show(&mut timer, leds, 100);
    }
}
