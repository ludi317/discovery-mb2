#![no_main]
#![no_std]

use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use cortex_m::asm;
use cortex_m::interrupt::{free as interrupt_free, Mutex};
use cortex_m_rt::entry;
use critical_section_lock_mut::LockMut;
use embedded_hal::{delay::DelayNs, digital::OutputPin};
use panic_rtt_target as _;
use rtt_target::{rprintln, rtt_init_print};

use microbit::{
    display::nonblocking::{Display, GreyscaleImage},
    hal::{
        gpio::{self, Pin, Output, PushPull},
        gpiote,
        pac::{self, interrupt, TIMER0, TIMER1},
        Timer,
    },
};

// Global state shared between interrupts and main
static CURRENT_VALUE: AtomicU8 = AtomicU8::new(1);
static TICK_COUNTER: AtomicU32 = AtomicU32::new(0);
static GPIOTE_PERIPHERAL: LockMut<gpiote::Gpiote> = LockMut::new();
static DISPLAY: Mutex<RefCell<Option<Display<TIMER1>>>> = Mutex::new(RefCell::new(None));
static SPEAKER: Mutex<RefCell<Option<Pin<Output<PushPull>>>>> = Mutex::new(RefCell::new(None));
static BEEP_TIMER: Mutex<RefCell<Option<Timer<TIMER0>>>> = Mutex::new(RefCell::new(None));

// Sound configuration
const BEEP_HZ: u32 = 440; // A4 note
const BEEP_DURATION_MS: u32 = 100;

// GPIOTE interrupt for button presses
#[interrupt]
fn GPIOTE() {
    GPIOTE_PERIPHERAL.with_lock(|gpiote| {
        let tick = TICK_COUNTER.load(Ordering::Relaxed);

        // Check if button A triggered the interrupt (channel 0)
        if gpiote.channel0().is_event_triggered() {
            rprintln!("Button A pressed! Generating number...");

            // Generate random number using tick counter
            let random_seed = (tick.wrapping_mul(7).wrapping_add(13)) as u8;
            let value = (random_seed % 6) + 1; // 1-6
            CURRENT_VALUE.store(value, Ordering::Relaxed);

            rprintln!("Result: {}", value);

            // Update display
            update_display(value);

            // Play beep
            play_beep_from_interrupt();

            gpiote.channel0().reset_events();
        }

        // Check if button B triggered the interrupt (channel 1)
        if gpiote.channel1().is_event_triggered() {
            rprintln!("Button B pressed! Generating number...");

            // Generate random number using different multiplier
            let random_seed = (tick.wrapping_mul(11).wrapping_add(17)) as u8;
            let value = (random_seed % 6) + 1; // 1-6
            CURRENT_VALUE.store(value, Ordering::Relaxed);

            rprintln!("Result: {}", value);

            // Update display
            update_display(value);

            // Play beep
            play_beep_from_interrupt();

            gpiote.channel1().reset_events();
        }
    });
}

// TIMER1 interrupt for display refresh
#[interrupt]
fn TIMER1() {
    interrupt_free(|cs| {
        if let Some(display) = DISPLAY.borrow(cs).borrow_mut().as_mut() {
            display.handle_display_event();
        }
    });
}

fn play_beep_from_interrupt() {
    interrupt_free(|cs| {
        if let (Some(speaker), Some(timer)) = (
            SPEAKER.borrow(cs).borrow_mut().as_mut(),
            BEEP_TIMER.borrow(cs).borrow_mut().as_mut(),
        ) {
            let period_us = 1_000_000 / BEEP_HZ;
            let cycles = (BEEP_DURATION_MS * 1000) / period_us;

            for _ in 0..cycles {
                let _ = speaker.set_high();
                timer.delay_us(period_us / 2);
                let _ = speaker.set_low();
                timer.delay_us(period_us / 2);
            }
        }
    });
}

fn update_display(value: u8) {
    let pattern = get_dice_pattern(value);
    let image = GreyscaleImage::new(&pattern);

    interrupt_free(|cs| {
        if let Some(display) = DISPLAY.borrow(cs).borrow_mut().as_mut() {
            display.show(&image);
        }
    });
}

#[entry]
fn main() -> ! {
    rtt_init_print!();
    let board = microbit::Board::take().unwrap();

    // Set up non-blocking display with TIMER1
    let display = Display::new(board.TIMER1, board.display_pins);
    interrupt_free(|cs| {
        *DISPLAY.borrow(cs).borrow_mut() = Some(display);
    });
    unsafe { pac::NVIC::unmask(pac::Interrupt::TIMER1) };

    // Set up speaker in global state
    let speaker_pin = board.speaker_pin.into_push_pull_output(gpio::Level::Low).degrade();
    interrupt_free(|cs| {
        SPEAKER.borrow(cs).replace(Some(speaker_pin));
    });

    // Set up timer for beeps in global state
    let timer0 = Timer::new(board.TIMER0);
    interrupt_free(|cs| {
        BEEP_TIMER.borrow(cs).replace(Some(timer0));
    });

    // Set up buttons as floating inputs
    let button_a = board.buttons.button_a.into_floating_input();
    let button_b = board.buttons.button_b.into_floating_input();

    // Set up GPIOTE for button interrupts
    let gpiote = gpiote::Gpiote::new(board.GPIOTE);

    // Configure channel 0 for button A (high-to-low = button press)
    let channel0 = gpiote.channel0();
    channel0
        .input_pin(&button_a.degrade())
        .hi_to_lo()
        .enable_interrupt();
    channel0.reset_events();

    // Configure channel 1 for button B (high-to-low = button press)
    let channel1 = gpiote.channel1();
    channel1
        .input_pin(&button_b.degrade())
        .hi_to_lo()
        .enable_interrupt();
    channel1.reset_events();

    GPIOTE_PERIPHERAL.init(gpiote);

    // Enable GPIOTE interrupts
    unsafe { pac::NVIC::unmask(pac::Interrupt::GPIOTE) };
    pac::NVIC::unpend(pac::Interrupt::GPIOTE);

    rprintln!("Random Number Generator Ready!");
    rprintln!("Press button A or B to generate a number 1-6!");
    rprintln!("Using interrupt-driven button handling and display!");

    // Show initial value
    update_display(1);

    loop {
        // Increment tick counter for randomness
        TICK_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Sleep - display updates automatically via TIMER1 interrupt
        asm::wfi();
    }
}

// Get LED pattern for numbers 1-6
fn get_dice_pattern(value: u8) -> [[u8; 5]; 5] {
    match value {
        1 => [
            [0, 0, 9, 0, 0],
            [0, 9, 9, 0, 0],
            [0, 0, 9, 0, 0],
            [0, 0, 9, 0, 0],
            [0, 9, 9, 9, 0],
        ],
        2 => [
            [0, 9, 9, 9, 0],
            [0, 0, 0, 9, 0],
            [0, 9, 9, 9, 0],
            [0, 9, 0, 0, 0],
            [0, 9, 9, 9, 0],
        ],
        3 => [
            [0, 9, 9, 9, 0],
            [0, 0, 0, 9, 0],
            [0, 9, 9, 9, 0],
            [0, 0, 0, 9, 0],
            [0, 9, 9, 9, 0],
        ],
        4 => [
            [0, 9, 0, 9, 0],
            [0, 9, 0, 9, 0],
            [0, 9, 9, 9, 0],
            [0, 0, 0, 9, 0],
            [0, 0, 0, 9, 0],
        ],
        5 => [
            [0, 9, 9, 9, 0],
            [0, 9, 0, 0, 0],
            [0, 9, 9, 9, 0],
            [0, 0, 0, 9, 0],
            [0, 9, 9, 9, 0],
        ],
        6 => [
            [0, 9, 9, 9, 0],
            [0, 9, 0, 0, 0],
            [0, 9, 9, 9, 0],
            [0, 9, 0, 9, 0],
            [0, 9, 9, 9, 0],
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
