#![no_main]
#![no_std]

use cortex_m::asm;
use cortex_m_rt::entry;
use critical_section_lock_mut::LockMut;
use embedded_hal::delay::DelayNs;
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

// Resources needed for beeping
struct BeepResources {
    pwm: Pwm<PWM0>,
    timer: Timer<TIMER0>,
}

// Global state shared between interrupts and main
static GPIOTE_PERIPHERAL: LockMut<gpiote::Gpiote> = LockMut::new();
static mut DISPLAY: Option<Display<TIMER1>> = None;
static mut BEEP_RESOURCES: Option<BeepResources> = None;
static mut RNG: Option<Rng> = None;

// Sound configuration
const BEEP_DURATION_MS: u32 = 100;

// GPIOTE interrupt for Button A or B presses
#[interrupt]
fn GPIOTE() {
    GPIOTE_PERIPHERAL.with_lock(|gpiote| {
        // SAFETY: RNG is only accessed from GPIOTE (within critical section) and main (before interrupts enabled).
        let rand_val = unsafe {
            let random_byte = RNG.as_mut().unwrap().random_u8();
            (random_byte % 6) + 1
        };

        update_display(rand_val);

        gpiote.channel0().reset_events();
        gpiote.channel1().reset_events();
    });

    play_beep_from_interrupt();
}

// TIMER1 interrupt for display refresh
#[interrupt]
fn TIMER1() {
    // SAFETY: DISPLAY is written in main (before interrupts) and GPIOTE (via with_lock() critical section).
    // DISPLAY is read from TIMER1, but TIMER1 is disabled during GPIOTE's critical section.
    unsafe {
        if let Some(display) = DISPLAY.as_mut() {
            display.handle_display_event();
        }
    }
}

fn play_beep_from_interrupt() {
    // SAFETY: BEEP_RESOURCES is only accessed from GPIOTE handler (non-reentrant) and main (before interrupts enabled).
    // TIMER1 can preempt this function but doesn't access BEEP_RESOURCES.
    unsafe {
        if let Some(resources) = BEEP_RESOURCES.as_mut() {
            // Turn on sound by setting 50% duty cycle
            resources.pwm.set_duty_on(Channel::C0, 18182);

            // Wait for beep duration
            resources.timer.delay_ms(BEEP_DURATION_MS);

            // Turn off sound by setting 0% duty cycle
            resources.pwm.set_duty_on(Channel::C0, 0);
        }
    }
}

fn update_display(value: u8) {
    let pattern = get_dice_pattern(value);
    let image = BitImage::new(&pattern);
    // SAFETY: DISPLAY is written in main (before interrupts) and GPIOTE (via with_lock() critical section).
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

    // Set up beep resources with PWM
    let beep_resources = BeepResources {
        pwm,
        timer: Timer::new(board.TIMER0),
    };

    // Set up hardware RNG
    let rng = Rng::new(board.RNG);

    // SAFETY: One-time initialization before any interrupts are enabled.
    unsafe {
        DISPLAY = Some(display);
        BEEP_RESOURCES = Some(beep_resources);
        RNG = Some(rng);
    }

    // SAFETY: RNG is initialized above, no interrupts enabled yet.
    let rand_val = unsafe {
        let random_byte = RNG.as_mut().unwrap().random_u8();
        (random_byte % 6) + 1
    };

    update_display(rand_val);

    // Enable TIMER1 interrupt for display refresh with high priority
    unsafe {
        let mut nvic = cortex_m::Peripherals::steal().NVIC;
        // nRF52833 has 3 priority bits in upper positions, so shift: 1 << (8-3) = 32
        nvic.set_priority(pac::Interrupt::TIMER1, 32); // Priority level 1 (0x20)
        pac::NVIC::unmask(pac::Interrupt::TIMER1);
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

    // Enable GPIOTE interrupts with lower priority
    unsafe {
        let mut nvic = cortex_m::Peripherals::steal().NVIC;
        // nRF52833 has 3 priority bits in upper positions, so shift: 2 << (8-3) = 64
        nvic.set_priority(pac::Interrupt::GPIOTE, 64); // Priority level 2 (0x40)
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
