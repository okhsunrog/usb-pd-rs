#![no_std]
#![no_main]

use {
    defmt::{debug, info},
    defmt_rtt as _,
    embassy_executor::Spawner,
    embassy_stm32::{
        Config, bind_interrupts,
        i2c::{self, I2c},
        peripherals,
        time::Hertz,
    },
    fusb302b::Fusb302b,
    panic_probe as _,
    uom::si::{electric_current::milliampere, electric_potential::millivolt},
    usb_pd::{
        messages::pdo::PowerDataObject,
        sink::{Driver, Event, Request, Sink},
    },
};

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    I2C2_EV => i2c::EventInterruptHandler<peripherals::I2C2>;
    I2C2_ER => i2c::ErrorInterruptHandler<peripherals::I2C2>;
});

// Wait for an event on the sink, then form a request from it, if applicable.
async fn receive_request<DRIVER: Driver>(
    sink: &mut Sink<DRIVER>,
) -> Result<Option<Request>, DRIVER::RxError> {
    if let Some(event) = sink.wait_for_event().await? {
        Ok(handle_event(event))
    } else {
        Ok(None)
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    defmt::info!("starting");

    let p = embassy_stm32::init(Config::default());

    let i2c1 = I2c::new(
        p.I2C1,
        p.PB6,
        p.PB7,
        Irqs,
        p.DMA1_CH6,
        p.DMA1_CH7,
        Hertz(400_000),
        Default::default(),
    );

    let i2c2 = I2c::new(
        p.I2C2,
        p.PB10,
        p.PB11,
        Irqs,
        p.DMA1_CH4,
        p.DMA1_CH5,
        Hertz(400_000),
        Default::default(),
    );

    let mut pd1 = Sink::new(Fusb302b::new(i2c1));
    pd1.init().await;

    let mut pd2 = Sink::new(Fusb302b::new(i2c2));
    pd2.init().await;

    defmt::info!("PD init done");

    loop {
        if let Ok(Some(req)) = receive_request(&mut pd1).await {
            _ = pd1.request(req).await;
        }

        if let Ok(Some(req)) = receive_request(&mut pd2).await {
            _ = pd2.request(req).await;
        }
    }
}

// #[embassy_executor::task]
// async fn blink(mut led: Output<'static, PB3>) {
//     let mut ticker = Ticker::every(Duration::from_hz(2));
//     loop {
//         led.set_high();
//         ticker.next().await;
//         led.set_low();
//         ticker.next().await;
//     }
// }

fn handle_event(event: Event) -> Option<Request> {
    match event {
        Event::SourceCapabilitiesChanged(caps) => {
            // info!("Capabilities changed: {}", caps.len());

            // Take maximum voltage
            let (index, supply) = caps
                .pdos()
                .iter()
                .enumerate()
                .filter_map(|(i, cap)| {
                    if let PowerDataObject::FixedSupply(supply) = cap {
                        debug!(
                            "supply @ {}: {}mV {}mA",
                            i,
                            supply.voltage().get::<millivolt>(),
                            supply.max_current().get::<milliampere>(),
                        );
                        Some((i, supply))
                    } else {
                        None
                    }
                })
                .max_by(|(_, x), (_, y)| x.raw_voltage().cmp(&y.raw_voltage()))
                .unwrap();

            let req = Request::RequestPower {
                index,
                current: supply.raw_max_current(),
            };

            info!("requesting {}", req);

            return Some(req);
        }
        Event::PowerReady => info!("power ready"),
        Event::ProtocolChanged => info!("protocol changed"),
        Event::PowerAccepted => info!("power accepted"),
        Event::PowerRejected => info!("power rejected"),
        _ => todo!(),
    }

    None
}
