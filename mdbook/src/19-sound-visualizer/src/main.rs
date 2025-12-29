#![deny(unsafe_code)]
#![no_main]
#![no_std]

use cortex_m_rt::entry;
use embedded_hal::delay::DelayNs;
use panic_rtt_target as _;
use rtt_target::{rprintln, rtt_init_print};

use microbit::{
    display::blocking::Display,
    hal::{
        gpio,
        saadc::{Saadc, SaadcConfig, Resolution, Gain, Reference, Time},
        Timer,
    },
};

#[entry]
fn main() -> ! {
    rtt_init_print!();
    let board = microbit::Board::take().unwrap();

    let mut timer = Timer::new(board.TIMER0);
    let mut display = Display::new(board.display_pins);

    // Configure SAADC for microphone input
    // Microphone is on pin P0.05 (AIN3) with 1.65V bias
    let saadc_config = SaadcConfig {
        resolution: Resolution::_12BIT,
        oversample: microbit::hal::saadc::Oversample::BYPASS,
        reference: Reference::VDD1_4, // Internal reference (VDD/4 = 0.825V)
        gain: Gain::GAIN1_2,           // Increased gain to 1/2 for more sensitivity
        resistor: microbit::hal::saadc::Resistor::BYPASS,
        time: Time::_10US,
    };

    let mut saadc = Saadc::new(board.ADC, saadc_config);

    // Enable microphone by setting run pin HIGH
    let _mic_run = board.microphone_pins.mic_run.into_push_pull_output(gpio::Level::High);

    // Use microphone pin (AIN3)
    let mut mic_pin = board.microphone_pins.mic_in.into_floating_input();

    rprintln!("Sound Visualizer Ready!");
    rprintln!("Microphone enabled!");
    rprintln!("Make some noise to see the LED visualization!");

    // Configuration constants
    const SAMPLE_COUNT: usize = 32; // Number of samples to average
    const QUIET_THRESHOLD: i16 = 5; // Minimum amplitude to register sound (very sensitive for speech)
    const MAX_LEVEL: i16 = 150; // Maximum expected sound level for scaling

    // Track baseline (DC offset from microphone bias)
    let mut baseline: i32 = 0;
    let mut initialized = false;

    loop {
        // Take multiple samples and calculate amplitude
        let mut sum: i32 = 0;
        let mut min_val: i16 = i16::MAX;
        let mut max_val: i16 = i16::MIN;

        for _ in 0..SAMPLE_COUNT {
            // Read microphone sample
            let sample = saadc.read_channel(&mut mic_pin).unwrap_or(0);
            sum += sample as i32;

            // Track min/max for amplitude calculation
            if sample < min_val {
                min_val = sample;
            }
            if sample > max_val {
                max_val = sample;
            }

            timer.delay_us(100u32); // Small delay between samples
        }

        // Calculate average (DC component)
        let average = sum / SAMPLE_COUNT as i32;

        // Initialize baseline on first run
        if !initialized {
            baseline = average;
            initialized = true;
            rprintln!("Baseline initialized: {}", baseline);
        }

        // Update baseline slowly to track DC drift
        baseline = (baseline * 15 + average) / 16;

        // Calculate amplitude (peak-to-peak)
        let amplitude = (max_val - min_val).abs();

        // Scale amplitude to LED levels (0-5)
        let level = if amplitude < QUIET_THRESHOLD {
            0
        } else {
            let scaled = ((amplitude - QUIET_THRESHOLD) as i32 * 5) / MAX_LEVEL as i32;
            scaled.min(5).max(0) as usize
        };

        // Debug output - show raw values and amplitude
        rprintln!(
            "Min: {} | Max: {} | Amp: {} | Avg: {} | Baseline: {} | Level: {}",
            min_val,
            max_val,
            amplitude,
            average,
            baseline,
            level
        );

        // Create visualization pattern
        let pattern = create_visualizer_pattern(level);

        // Display the pattern
        display.show(&mut timer, pattern, 20);
    }
}

// Create LED pattern based on sound level (0-5)
fn create_visualizer_pattern(level: usize) -> [[u8; 5]; 5] {
    match level {
        0 => {
            // Silence - single center dot
            [
                [0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0],
                [0, 0, 1, 0, 0],
                [0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0],
            ]
        }
        1 => {
            // Quiet - small cross
            [
                [0, 0, 0, 0, 0],
                [0, 0, 1, 0, 0],
                [0, 1, 1, 1, 0],
                [0, 0, 1, 0, 0],
                [0, 0, 0, 0, 0],
            ]
        }
        2 => {
            // Moderate - larger cross
            [
                [0, 0, 1, 0, 0],
                [0, 1, 1, 1, 0],
                [1, 1, 1, 1, 1],
                [0, 1, 1, 1, 0],
                [0, 0, 1, 0, 0],
            ]
        }
        3 => {
            // Loud - diamond pattern
            [
                [0, 0, 1, 0, 0],
                [0, 1, 0, 1, 0],
                [1, 0, 1, 0, 1],
                [0, 1, 0, 1, 0],
                [0, 0, 1, 0, 0],
            ]
        }
        4 => {
            // Very loud - expanding rings
            [
                [1, 1, 1, 1, 1],
                [1, 0, 0, 0, 1],
                [1, 0, 1, 0, 1],
                [1, 0, 0, 0, 1],
                [1, 1, 1, 1, 1],
            ]
        }
        _ => {
            // Maximum - all LEDs
            [
                [1, 1, 1, 1, 1],
                [1, 1, 1, 1, 1],
                [1, 1, 1, 1, 1],
                [1, 1, 1, 1, 1],
                [1, 1, 1, 1, 1],
            ]
        }
    }
}
