#![no_main]
#![no_std]

use cortex_m::asm;
use cortex_m_rt::entry;
use critical_section_lock_mut::LockMut;
use panic_rtt_target as _;

use microbit::{
    display::nonblocking::{Display, BitImage},
    hal::{
        gpiote,
        pac::{self, interrupt, PWM0, TIMER0, TIMER1},
        pwm::{Pwm, Channel},
        rng::Rng,
        Timer,
    },
};

// Global state shared between interrupts and main
static GPIOTE_PERIPHERAL: LockMut<gpiote::Gpiote> = LockMut::new();
static mut DISPLAY: Option<Display<TIMER1>> = None;
static mut BEEP_PWM: Option<Pwm<PWM0>> = None;
static mut BEEP_TIMER: Option<Timer<TIMER0>> = None;
static mut RNG: Option<Rng> = None;

// Sound configuration
const BEEP_DURATION_MS: u32 = 100;

// GPIOTE interrupt for Button A or B presses
#[interrupt]
fn GPIOTE() {

    // SAFETY: RNG is only accessed from GPIOTE (not re-entrant).
    let rand_val = unsafe {
        let random_byte = RNG.as_mut().unwrap().random_u8();
        (random_byte % 6) + 1
    };

    GPIOTE_PERIPHERAL.with_lock(|gpiote| {
        // all interrupts are disabled in the critical section
        update_display(rand_val);

        gpiote.channel0().reset_events();
        gpiote.channel1().reset_events();
    });

    // play beep outside critical section
    // SAFETY: BEEP_PWM and BEEP_TIMER accessed from GPIOTE (non-reentrant) and TIMER0 (non-reentrant).
    // Same priority means they cannot preempt each other. Sequential execution.
    unsafe {
        // turn on beep
        BEEP_PWM.as_mut().unwrap().set_duty_on(Channel::C0, 18182);

        // turn on beep timer
        let timer = BEEP_TIMER.as_mut().unwrap();
        timer.start(BEEP_DURATION_MS * 1000u32);
        timer.enable_interrupt();
    }

}

// TIMER0 interrupt for beep duration
#[interrupt]
fn TIMER0() {
    // SAFETY: BEEP_PWM and BEEP_TIMER accessed from GPIOTE (non-reentrant) and TIMER0 (non-reentrant).
    // Same priority means they cannot preempt each other. Sequential execution.
    unsafe {
        // turn off beep and timer
        BEEP_PWM.as_mut().unwrap().set_duty_on(Channel::C0, 0);
        BEEP_TIMER.as_mut().unwrap().disable_interrupt();
    }
}

// TIMER1 interrupt for LED rendering
#[interrupt]
fn TIMER1() {
    // SAFETY: DISPLAY is written in GPIOTE (via with_lock() critical section).
    // DISPLAY is read from TIMER1, but TIMER1 is disabled during GPIOTE's critical section.
    unsafe {
        if let Some(display) = DISPLAY.as_mut() {
            display.handle_display_event();
        }
    }
}

fn update_display(value: u8) {
    let pattern = get_dice_pattern(value);
    let image = BitImage::new(&pattern);
    // SAFETY: DISPLAY is written in GPIOTE (via with_lock() critical section).
    // DISPLAY is read from TIMER1, but TIMER1 is disabled during GPIOTE's critical section.
    unsafe {
        if let Some(display) = DISPLAY.as_mut() {
            display.show(&image);
        }
    }
}

#[entry]
fn main() -> ! {
    let board = microbit::Board::take().unwrap();

    // Set up non-blocking display with TIMER1
    let display = Display::new(board.TIMER1, board.display_pins);

    // Set up PWM for audio on speaker pin
    let pwm = Pwm::new(board.PWM0);

    // Configure PWM: 440Hz tone with 50% duty cycle
    // PWM frequency = 16MHz / prescaler / max_duty
    // For 440Hz: max_duty = 16_000_000 / 440 ≈ 36364
    pwm.set_prescaler(microbit::hal::pwm::Prescaler::Div1);
    pwm.set_max_duty(36364);

    let speaker_pin = board.speaker_pin.into_push_pull_output(microbit::hal::gpio::Level::Low).degrade();
    pwm.set_output_pin(Channel::C0, speaker_pin);
    pwm.set_duty_on(Channel::C0, 0); // Start silent (0% duty cycle)
    pwm.enable(); // Enable PWM but with 0 duty = no sound

    // Set up timer for beep duration
    let beep_timer = Timer::new(board.TIMER0);

    // Set up hardware RNG
    let rng = Rng::new(board.RNG);

    // SAFETY: One-time initialization before any interrupts are enabled.
    unsafe {
        DISPLAY = Some(display);
        BEEP_PWM = Some(pwm);
        BEEP_TIMER = Some(beep_timer);
        RNG = Some(rng);
    }

    // SAFETY: RNG is initialized above, no interrupts enabled yet.
    let rand_val = unsafe {
        let random_byte = RNG.as_mut().unwrap().random_u8();
        (random_byte % 6) + 1
    };

    // SAFETY: No interrupts enabled.
    update_display(rand_val);

    // Enable TIMER1 interrupt for display refresh
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::TIMER1);
    }

    // Enable TIMER0 interrupt for beep duration
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::TIMER0);
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

    // Enable GPIOTE interrupts
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::GPIOTE);
    }
    pac::NVIC::unpend(pac::Interrupt::GPIOTE);

    loop {
        // Sleep until GPIOTE or TIMER1 interrupts
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
