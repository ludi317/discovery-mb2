#![deny(unsafe_code)]
#![no_main]
#![no_std]

use cortex_m_rt::entry;
use embedded_hal::{delay::DelayNs, digital::{InputPin, OutputPin}};
use panic_rtt_target as _;
use rtt_target::{rprintln, rtt_init_print};

use microbit::{
    display::blocking::Display,
    hal::{gpio, Timer},
};

#[entry]
fn main() -> ! {
    rtt_init_print!();
    let board = microbit::Board::take().unwrap();

    let mut timer = Timer::new(board.TIMER0);
    let mut display = Display::new(board.display_pins);
    let mut speaker_pin = board.speaker_pin.into_push_pull_output(gpio::Level::Low);

    // Initialize buttons
    let mut button_a = board.buttons.button_a;
    let mut button_b = board.buttons.button_b;

    // Sound configuration
    const BEEP_HZ: u32 = 440; // A4 note
    const BEEP_DURATION_MS: u32 = 100;

    let mut current_value: u8 = 1;
    let mut tick_counter: u32 = 0;

    // Button state tracking to detect press (not hold)
    let mut button_a_pressed = false;
    let mut button_b_pressed = false;

    rprintln!("Random Number Generator Ready!");
    rprintln!("Press button A or B to generate a number 1-6!");

    loop {
        tick_counter += 1;

        // Check button A
        if button_a.is_low().unwrap() {
            if !button_a_pressed {
                button_a_pressed = true;
                rprintln!("Button A pressed! Generating number...");

                // Generate "random" number using tick counter
                // Use tick_counter as pseudo-random seed
                let random_seed = (tick_counter * 7 + 13) as u8; // Simple PRNG
                current_value = (random_seed % 6) + 1; // 1-6

                rprintln!("Result: {}", current_value);

                // Play beep
                play_beep(&mut speaker_pin, &mut timer, BEEP_HZ, BEEP_DURATION_MS);
            }
        } else {
            button_a_pressed = false;
        }

        // Check button B
        if button_b.is_low().unwrap() {
            if !button_b_pressed {
                button_b_pressed = true;
                rprintln!("Button B pressed! Generating number...");

                // Generate "random" number using tick counter
                // Use different multiplier for variety
                let random_seed = (tick_counter * 11 + 17) as u8; // Simple PRNG
                current_value = (random_seed % 6) + 1; // 1-6

                rprintln!("Result: {}", current_value);

                // Play beep
                play_beep(&mut speaker_pin, &mut timer, BEEP_HZ, BEEP_DURATION_MS);
            }
        } else {
            button_b_pressed = false;
        }

        // Display current number value
        let pattern = get_dice_pattern(current_value);
        display.show(&mut timer, pattern, 50);

        timer.delay_ms(1u32);
    }
}

// Play a simple beep tone
fn play_beep(speaker: &mut dyn OutputPin<Error = core::convert::Infallible>, timer: &mut Timer<microbit::pac::TIMER0>, frequency_hz: u32, duration_ms: u32) {
    let period_us = 1_000_000 / frequency_hz;
    let cycles = (duration_ms * 1000) / period_us;

    for _ in 0..cycles {
        speaker.set_high().unwrap();
        timer.delay_us(period_us / 2);
        speaker.set_low().unwrap();
        timer.delay_us(period_us / 2);
    }
}

// Get LED pattern for numbers 1-6
fn get_dice_pattern(value: u8) -> [[u8; 5]; 5] {
    match value {
        1 => [
            [0, 0, 1, 0, 0],
            [0, 1, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 1, 1, 1, 0],
        ],
        2 => [
            [0, 1, 1, 1, 0],
            [0, 0, 0, 1, 0],
            [0, 1, 1, 1, 0],
            [0, 1, 0, 0, 0],
            [0, 1, 1, 1, 0],
        ],
        3 => [
            [0, 1, 1, 1, 0],
            [0, 0, 0, 1, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 1, 0],
            [0, 1, 1, 1, 0],
        ],
        4 => [
            [0, 1, 0, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 1, 0],
            [0, 0, 0, 1, 0],
        ],
        5 => [
            [0, 1, 1, 1, 0],
            [0, 1, 0, 0, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 1, 0],
            [0, 1, 1, 1, 0],
        ],
        6 => [
            [0, 1, 1, 1, 0],
            [0, 1, 0, 0, 0],
            [0, 1, 1, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 1, 1, 0],
        ],
        _ => [
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
        ],
    }
}
