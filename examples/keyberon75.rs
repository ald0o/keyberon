#![no_main]
#![no_std]

use embedded_hal::digital::v2::{InputPin, OutputPin};
use generic_array::typenum::{U15, U5};
use keyberon::action::Action::{self, *};
use keyberon::action::{d, k, l, m};
use keyberon::debounce::Debouncer;
use keyberon::impl_heterogenous_array;
use keyberon::key_code::KeyCode::*;
use keyberon::layout::Layout;
use keyberon::matrix::{Matrix, PressedKeys};
use panic_semihosting as _;
use rtfm::app;
use stm32_usbd::{UsbBus, UsbBusType};
use stm32f1xx_hal::gpio::{gpioa::*, gpiob::*, Input, Output, PullUp, PushPull};
use stm32f1xx_hal::prelude::*;
use stm32f1xx_hal::{gpio, pac, timer};
use usb_device::bus::UsbBusAllocator;
use usb_device::class::UsbClass as _;
use void::{ResultVoidExt, Void};

type UsbClass = keyberon::Class<'static, UsbBusType, Leds>;
type UsbDevice = keyberon::Device<'static, UsbBusType>;

pub struct Leds {
    caps_lock: gpio::gpioc::PC13<gpio::Output<gpio::PushPull>>,
}
impl keyberon::keyboard::Leds for Leds {
    fn caps_lock(&mut self, status: bool) {
        if status {
            self.caps_lock.set_low().void_unwrap()
        } else {
            self.caps_lock.set_high().void_unwrap()
        }
    }
}

pub struct Cols(
    pub PB12<Input<PullUp>>,
    pub PB13<Input<PullUp>>,
    pub PB14<Input<PullUp>>,
    pub PB15<Input<PullUp>>,
    pub PA8<Input<PullUp>>,
    pub PA9<Input<PullUp>>,
    pub PA10<Input<PullUp>>,
    pub PB5<Input<PullUp>>,
    pub PB6<Input<PullUp>>,
    pub PB7<Input<PullUp>>,
    pub PB8<Input<PullUp>>,
    pub PB9<Input<PullUp>>,
    pub PA6<Input<PullUp>>,
    pub PA5<Input<PullUp>>,
    pub PA4<Input<PullUp>>,
);
impl_heterogenous_array! {
    Cols,
    dyn InputPin<Error = Void>,
    U15,
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
}

pub struct Rows(
    pub PB11<Output<PushPull>>,
    pub PB10<Output<PushPull>>,
    pub PB1<Output<PushPull>>,
    pub PB0<Output<PushPull>>,
    pub PA7<Output<PushPull>>,
);
impl_heterogenous_array! {
    Rows,
    dyn OutputPin<Error = Void>,
    U5,
    [0, 1, 2, 3, 4]
}

const CUT: Action = m(&[LShift, Delete]);
const COPY: Action = m(&[LCtrl, Insert]);
const PASTE: Action = m(&[LShift, Insert]);

#[rustfmt::skip]
pub static LAYERS: keyberon::layout::Layers = &[
    &[
        &[k(Grave),   k(Kb1),k(Kb2),k(Kb3), k(Kb4),  k(Kb5),k(KpMinus),k(KpSlash),k(KpAsterisk),k(Kb6),   k(Kb7),  k(Kb8), k(Kb9),  k(Kb0),   k(Minus)   ],
        &[k(Tab),     k(Q),  k(W),  k(E),   k(R),    k(T),     k(Kp7), k(Kp8),    k(Kp9),       k(Y),     k(U),    k(I),   k(O),    k(P),     k(LBracket)],
        &[k(RBracket),k(A),  k(S),  k(D),   k(F),    k(G),     k(Kp4), k(Kp5),    k(Kp6),       k(H),     k(J),    k(K),   k(L),    k(SColon),k(Quote)   ],
        &[k(Equal),   k(Z),  k(X),  k(C),   k(V),    k(B),     k(Kp1), k(Kp2),    k(Kp3),       k(N),     k(M),    k(Comma),k(Dot), k(Slash), k(Bslash)  ],
        &[k(LCtrl),   l(1), k(LGui),k(LAlt),k(Space),k(LShift),k(Kp0), k(KpDot),  k(KpEnter),   k(RShift),k(Enter),k(RAlt),k(BSpace),k(Escape),k(RCtrl)  ],
    ], &[
        &[k(F1),      k(F2),    k(F3),     k(F4),k(F5),k(F6),Trans,Trans,Trans,k(F7),k(F8),  k(F9),  k(F10), k(F11),  k(F12)   ],
        &[k(Escape),  Trans,    Trans,     Trans,Trans,Trans,Trans,Trans,Trans,Trans,Trans,  Trans,  Trans,  Trans,   k(PgUp)  ],
        &[d(0),       d(1),     k(NumLock),Trans,Trans,Trans,Trans,Trans,Trans,Trans,k(Left),k(Down),k(Up),  k(Right),k(PgDown)],
        &[k(CapsLock),k(Delete),CUT,       COPY, PASTE,Trans,Trans,Trans,Trans,Trans,Trans,  Trans,  k(Home),k(Up),   k(End)   ],
        &[Trans,      Trans,    Trans,     Trans,Trans,Trans,Trans,Trans,Trans,Trans,Trans,  Trans,  k(Left),k(Down), k(Right) ],
    ]
];

#[app(device = stm32f1xx_hal::pac)]
const APP: () = {
    static mut USB_DEV: UsbDevice = ();
    static mut USB_CLASS: UsbClass = ();
    static mut MATRIX: Matrix<Cols, Rows> = ();
    static mut DEBOUNCER: Debouncer<PressedKeys<U5, U15>> = ();
    static mut LAYOUT: Layout = Layout::new(LAYERS);
    static mut TIMER: timer::Timer<pac::TIM3> = ();

    #[init]
    fn init() -> init::LateResources {
        static mut USB_BUS: Option<UsbBusAllocator<UsbBusType>> = None;

        let mut flash = device.FLASH.constrain();
        let mut rcc = device.RCC.constrain();

        let clocks = rcc
            .cfgr
            .use_hse(8.mhz())
            .sysclk(48.mhz())
            .pclk1(24.mhz())
            .freeze(&mut flash.acr);

        let mut gpioa = device.GPIOA.split(&mut rcc.apb2);
        let mut gpiob = device.GPIOB.split(&mut rcc.apb2);
        let mut gpioc = device.GPIOC.split(&mut rcc.apb2);

        let mut led = gpioc.pc13.into_push_pull_output(&mut gpioc.crh);
        led.set_high().void_unwrap();
        let leds = Leds { caps_lock: led };

        let usb_dm = gpioa.pa11;
        let usb_dp = gpioa.pa12.into_floating_input(&mut gpioa.crh);

        *USB_BUS = Some(UsbBus::new(device.USB, (usb_dm, usb_dp)));
        let usb_bus = USB_BUS.as_ref().unwrap();

        let usb_class = keyberon::new_class(usb_bus, leds);
        let usb_dev = keyberon::new_device(usb_bus);

        let mut timer = timer::Timer::tim3(device.TIM3, 1.khz(), clocks, &mut rcc.apb1);
        timer.listen(timer::Event::Update);

        let matrix = Matrix::new(
            Cols(
                gpiob.pb12.into_pull_up_input(&mut gpiob.crh),
                gpiob.pb13.into_pull_up_input(&mut gpiob.crh),
                gpiob.pb14.into_pull_up_input(&mut gpiob.crh),
                gpiob.pb15.into_pull_up_input(&mut gpiob.crh),
                gpioa.pa8.into_pull_up_input(&mut gpioa.crh),
                gpioa.pa9.into_pull_up_input(&mut gpioa.crh),
                gpioa.pa10.into_pull_up_input(&mut gpioa.crh),
                gpiob.pb5.into_pull_up_input(&mut gpiob.crl),
                gpiob.pb6.into_pull_up_input(&mut gpiob.crl),
                gpiob.pb7.into_pull_up_input(&mut gpiob.crl),
                gpiob.pb8.into_pull_up_input(&mut gpiob.crh),
                gpiob.pb9.into_pull_up_input(&mut gpiob.crh),
                gpioa.pa6.into_pull_up_input(&mut gpioa.crl),
                gpioa.pa5.into_pull_up_input(&mut gpioa.crl),
                gpioa.pa4.into_pull_up_input(&mut gpioa.crl),
            ),
            Rows(
                gpiob.pb11.into_push_pull_output(&mut gpiob.crh),
                gpiob.pb10.into_push_pull_output(&mut gpiob.crh),
                gpiob.pb1.into_push_pull_output(&mut gpiob.crl),
                gpiob.pb0.into_push_pull_output(&mut gpiob.crl),
                gpioa.pa7.into_push_pull_output(&mut gpioa.crl),
            ),
        );

        init::LateResources {
            USB_DEV: usb_dev,
            USB_CLASS: usb_class,
            TIMER: timer,
            DEBOUNCER: Debouncer::new(PressedKeys::new(), PressedKeys::new(), 5),
            MATRIX: matrix.void_unwrap(),
        }
    }

    #[interrupt(priority = 2, resources = [USB_DEV, USB_CLASS])]
    fn USB_HP_CAN_TX() {
        usb_poll(&mut resources.USB_DEV, &mut resources.USB_CLASS);
    }

    #[interrupt(priority = 2, resources = [USB_DEV, USB_CLASS])]
    fn USB_LP_CAN_RX0() {
        usb_poll(&mut resources.USB_DEV, &mut resources.USB_CLASS);
    }

    #[interrupt(priority = 1, resources = [USB_CLASS, MATRIX, DEBOUNCER, LAYOUT, TIMER])]
    fn TIM3() {
        resources.TIMER.clear_update_interrupt_flag();

        if resources
            .DEBOUNCER
            .update(resources.MATRIX.get().void_unwrap())
        {
            let data = resources.DEBOUNCER.get();
            let report = resources.LAYOUT.report_from_pressed(data.iter_pressed());
            resources
                .USB_CLASS
                .lock(|k| k.device_mut().set_keyboard_report(report.clone()));
            while let Ok(0) = resources.USB_CLASS.lock(|k| k.write(report.as_bytes())) {}
        }
    }
};

fn usb_poll(usb_dev: &mut UsbDevice, keyboard: &mut UsbClass) {
    if usb_dev.poll(&mut [keyboard]) {
        keyboard.poll();
    }
}
