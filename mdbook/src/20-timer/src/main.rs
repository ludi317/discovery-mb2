#![no_main]
#![no_std]
#![allow(static_mut_refs)]

use cortex_m::asm;
use cortex_m_rt::entry;
use panic_rtt_target as _;

use microbit::{
    display::nonblocking::{Display, BitImage},
    hal::{
        gpiote,
        pac::{self, interrupt, PWM0, TIMER0, TIMER1, TIMER2, TIMER3},
        pwm::{Pwm, Channel},
        Timer,
    },
};

// Global state shared between interrupts and main
static mut GPIOTE_PERIPHERAL: Option<gpiote::Gpiote> = None;
static mut DISPLAY: Option<Display<TIMER1>> = None;
static mut BEEP_PWM: Option<Pwm<PWM0>> = None;
static mut COUNTDOWN_TIMER: Option<Timer<TIMER0>> = None;
static mut BEEP_TIMER: Option<Timer<TIMER2>> = None;
static mut BLINK_TIMER: Option<Timer<TIMER3>> = None;

// Timer state
static mut REMAINING_SECONDS: u32 = 10;
static mut TIMER_RUNNING: bool = false;
static mut NUM_BLINKS: u32 = 0;
const MAX_BLINKS: u32 = 10;
const COUNTDOWN_TIMER_INTERVAL: u32 = 1_000_000u32; // 1 second
const BLINK_TIMER_INTERVAL: u32 = 100 * 1_000u32; // 100 ms

// Sound configuration
const BEEP_DURATION_MS: u32 = 100;
const BEEP_HZ: u32 = 440; // A4 note
const PWM_MAX_DUTY: u16 = (16_000_000 / BEEP_HZ) as u16;
const PWM_DUTY_BEEP_ON: u16 = PWM_MAX_DUTY / 2; // 50% duty cycle
const PWM_DUTY_BEEP_OFF: u16 = 0; // Silent

// GPIOTE interrupt for Button A or B presses
#[interrupt]
fn GPIOTE() {
    // SAFETY: Interrupts are not re-entrant. Interrupts with same priority cannot preempt each other.
    // Sequential execution among interrupts.
    unsafe {
        let gpiote = GPIOTE_PERIPHERAL.as_mut().unwrap();

        // Check if Button A was pressed (toggle timer)
        if gpiote.channel0().is_event_triggered() {
            TIMER_RUNNING = !TIMER_RUNNING;
            let countdown_timer = COUNTDOWN_TIMER.as_mut().unwrap();

            // If starting the timer, enable countdown interrupt
            if TIMER_RUNNING && REMAINING_SECONDS > 0 {
                countdown_timer.disable_interrupt();
                pac::NVIC::unpend(pac::Interrupt::TIMER0);
                countdown_timer.start(COUNTDOWN_TIMER_INTERVAL);
                countdown_timer.enable_interrupt();
            } else {
                countdown_timer.disable_interrupt();
            }

            gpiote.channel0().reset_events();
        }

        // Check if Button B was pressed (reset timer)
        if gpiote.channel1().is_event_triggered() {
            REMAINING_SECONDS = 10;
            TIMER_RUNNING = false;

            // Stop the countdown timer
            let timer = COUNTDOWN_TIMER.as_mut().unwrap();
            timer.disable_interrupt();
            pac::NVIC::unpend(pac::Interrupt::TIMER0);

            // Update display
            update_display(REMAINING_SECONDS);

            gpiote.channel1().reset_events();
        }
    }
}

// TIMER0 interrupt for countdown
#[interrupt]
fn TIMER0() {
    // SAFETY: Sequential execution among interrupts.
    unsafe {
        let countdown_timer = COUNTDOWN_TIMER.as_mut().unwrap();

        REMAINING_SECONDS -= 1;
        update_display(REMAINING_SECONDS);

        if REMAINING_SECONDS == 0 {
            // Timer reached 0, beep and stop
            TIMER_RUNNING = false;
            countdown_timer.disable_interrupt();

            // Turn on beep
            BEEP_PWM.as_mut().unwrap().set_duty_on(Channel::C0, PWM_DUTY_BEEP_ON);

            // Start beep timer
            let beep_timer = BEEP_TIMER.as_mut().unwrap();
            beep_timer.start(BEEP_DURATION_MS * 1000u32);
            beep_timer.enable_interrupt();
            
            // Start blink timer
            let blink_timer = BLINK_TIMER.as_mut().unwrap();
            blink_timer.start(100 * 1000u32);
            blink_timer.enable_interrupt();
        } else {
            // Continue countdown
            countdown_timer.start(COUNTDOWN_TIMER_INTERVAL);
        }
    }
}

// TIMER1 interrupt for LED rendering
#[interrupt]
fn TIMER1() {
    // SAFETY: Sequential execution among interrupts.
    unsafe {
        DISPLAY.as_mut().unwrap().handle_display_event();
    }
}

// TIMER2 interrupt for beep duration
#[interrupt]
fn TIMER2() {
    // SAFETY: Sequential execution among interrupts.
    unsafe {
        BEEP_PWM.as_mut().unwrap().set_duty_on(Channel::C0, PWM_DUTY_BEEP_OFF);
        BEEP_TIMER.as_mut().unwrap().disable_interrupt();
    }
}

// TIMER3 interrupt for blinking
#[interrupt]
fn TIMER3() {
    // SAFETY: Sequential execution among interrupts.
    unsafe {
        let blink_timer = BLINK_TIMER.as_mut().unwrap();
        
        if NUM_BLINKS % 2 == 0 {
            update_display(11);
        } else {
            update_display(0);
        }

        NUM_BLINKS += 1;
        if NUM_BLINKS == MAX_BLINKS * 2 {
            blink_timer.disable_interrupt();
            NUM_BLINKS = 0;
        } else {
            blink_timer.start(BLINK_TIMER_INTERVAL);
        }
    }
}

fn update_display(seconds: u32) {
    let pattern = get_digit_pattern(seconds);
    let image = BitImage::new(&pattern);
    // SAFETY: Sequential execution among interrupts.
    unsafe {
        DISPLAY.as_mut().unwrap().show(&image);
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
    pwm.set_prescaler(microbit::hal::pwm::Prescaler::Div1);
    pwm.set_max_duty(PWM_MAX_DUTY);

    let speaker_pin = board.speaker_pin.into_push_pull_output(microbit::hal::gpio::Level::Low).degrade();
    pwm.set_output_pin(Channel::C0, speaker_pin);
    pwm.set_duty_on(Channel::C0, PWM_DUTY_BEEP_OFF); // Start silent
    pwm.enable();

    // Set up timer for countdown
    let countdown_timer = Timer::new(board.TIMER0);

    // Set up timer for beep duration
    let beep_timer = Timer::new(board.TIMER2);
    
    // Set up timer for blinks
    let blink_timer = Timer::new(board.TIMER3);

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

    // SAFETY: One-time initialization before any interrupts are enabled.
    unsafe {
        DISPLAY = Some(display);
        BEEP_PWM = Some(pwm);
        COUNTDOWN_TIMER = Some(countdown_timer);
        BEEP_TIMER = Some(beep_timer);
        BLINK_TIMER = Some(blink_timer);
        GPIOTE_PERIPHERAL = Some(gpiote);
    }

    // Display initial value
    update_display(10);

    // Enable the interrupts
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::TIMER1);
        pac::NVIC::unmask(pac::Interrupt::TIMER0);
        pac::NVIC::unmask(pac::Interrupt::TIMER2);
        pac::NVIC::unmask(pac::Interrupt::TIMER3);
        pac::NVIC::unmask(pac::Interrupt::GPIOTE);
    }

    pac::NVIC::unpend(pac::Interrupt::GPIOTE);

    loop {
        // Wait for Interrupt
        asm::wfi();
    }
}

// Get LED pattern for digits 0-9 (only 0-10 needed for timer)
fn get_digit_pattern(value: u32) -> [[u8; 5]; 5] {
    match value {
        0 => [
            [0, 1, 1, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 1, 1, 0],
        ],
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
        7 => [
            [0, 1, 1, 1, 0],
            [0, 0, 0, 1, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
        ],
        8 => [
            [0, 1, 1, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 1, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 1, 1, 0],
        ],
        9 => [
            [0, 1, 1, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 1, 0],
            [0, 1, 1, 1, 0],
        ],
        10 => [
            [1, 0, 1, 1, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 1, 1, 1],
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
