#![no_main]
#![no_std]

use cortex_m::asm;
use cortex_m_rt::entry;
use critical_section_lock_mut::LockMut;
use embedded_hal::{delay::DelayNs, digital::OutputPin};
use panic_rtt_target as _;

use microbit::{
    display::nonblocking::{Display, BitImage},
    hal::{
        gpio::{self, Pin, Output, PushPull},
        gpiote,
        pac::{self, interrupt, TIMER0, TIMER1},
        rng::Rng,
        Timer,
    },
};

// Resources needed for beeping
struct BeepResources {
    speaker: Pin<Output<PushPull>>,
    timer: Timer<TIMER0>,
}

// Global state shared between interrupts and main
static GPIOTE_PERIPHERAL: LockMut<gpiote::Gpiote> = LockMut::new();
static DISPLAY: LockMut<Display<TIMER1>> = LockMut::new();
static mut BEEP_RESOURCES: Option<BeepResources> = None;
static mut RNG: Option<Rng> = None;

// Sound configuration
const BEEP_HZ: u32 = 440; // A4 note
const BEEP_DURATION_MS: u32 = 100;

// GPIOTE interrupt for Button A or B presses
#[interrupt]
fn GPIOTE() {
    GPIOTE_PERIPHERAL.with_lock(|gpiote| {
        // SAFETY: RNG is only accessed from this GPIOTE interrupt handler.
        let rand_val = unsafe {
            let random_byte = RNG.as_mut().unwrap().random_u8();
            (random_byte % 6) + 1
        };

        update_display(rand_val);
        play_beep_from_interrupt();

        gpiote.channel0().reset_events();
        gpiote.channel1().reset_events();
    });
}

// TIMER1 interrupt for display refresh
#[interrupt]
fn TIMER1() {
    DISPLAY.with_lock(|display| {
        display.handle_display_event();
    });
}

fn play_beep_from_interrupt() {
    // SAFETY: BEEP_RESOURCES is only accessed from the GPIOTE interrupt handler.
    unsafe {
        if let Some(resources) = BEEP_RESOURCES.as_mut() {
            let period_us = 1_000_000 / BEEP_HZ;
            let cycles = (BEEP_DURATION_MS * 1000) / period_us;

            for _ in 0..cycles {
                let _ = resources.speaker.set_high();
                resources.timer.delay_us(period_us / 2);
                let _ = resources.speaker.set_low();
                resources.timer.delay_us(period_us / 2);
            }
        }
    }
}

fn update_display(value: u8) {
    let pattern = get_dice_pattern(value);
    let image = BitImage::new(&pattern);

    DISPLAY.with_lock(|display| {
        display.show(&image);
    });
}

#[entry]
fn main() -> ! {
    let board = microbit::Board::take().unwrap();

    // Set up non-blocking display with TIMER1
    let display = Display::new(board.TIMER1, board.display_pins);
    DISPLAY.init(display);
    unsafe { pac::NVIC::unmask(pac::Interrupt::TIMER1) };

    // Set up beep resources
    let beep_resources = BeepResources {
        speaker: board.speaker_pin.into_push_pull_output(gpio::Level::Low).degrade(),
        timer: Timer::new(board.TIMER0),
    };
    // SAFETY: GPIOTE interrupt (the only user of BEEP_RESOURCES) is not yet enabled.
    // One-time initialization.
    unsafe {
        BEEP_RESOURCES = Some(beep_resources);
    }

    // Set up hardware RNG
    let rng = Rng::new(board.RNG);
    // SAFETY: GPIOTE interrupt (the only user of RNG) is not yet enabled.
    // One-time initialization.
    unsafe {
        RNG = Some(rng);
    }

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

    // Show initial random value (before enabling GPIOTE interrupt)
    // SAFETY: RNG is initialized above, and GPIOTE interrupt is not yet enabled.
    let rand_val = unsafe {
        let random_byte = RNG.as_mut().unwrap().random_u8();
        (random_byte % 6) + 1
    };

    update_display(rand_val);

    // Enable GPIOTE interrupts
    unsafe { pac::NVIC::unmask(pac::Interrupt::GPIOTE) };
    pac::NVIC::unpend(pac::Interrupt::GPIOTE);

    loop {
        // Sleep - display updates automatically via TIMER1 interrupt
        asm::wfi();
    }
}

// Get LED pattern for numbers 1-6
// Returns simple binary: 0 = off, 1 = on
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
