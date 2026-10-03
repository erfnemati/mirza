//! Binds a test shortcut through the portal, then presses and holds it with a
//! virtual keyboard to see whether both press and release events arrive.
//! Run: cargo run -p mirza-hotkey --example portal_probe -- "Ctrl+Alt+Shift+F9"

#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use mirza_hotkey::{Binding, Event, portal};

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    let keys = std::env::args().nth(1).unwrap_or_else(|| "Ctrl+Alt+Shift+F9".into());
    portal::register_app("io.github.erfnemati.Mirza").await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let binding = Binding { id: "probe".into(), description: "Mirza portal probe".into(), keys: keys.clone() };
    let reg = match tokio::time::timeout(Duration::from_secs(60), portal::bind(&[binding], tx, None)).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return eprintln!("bind failed: {e}"),
        Err(_) => return eprintln!("bind timed out (a confirmation dialog may be waiting)"),
    };
    println!("bound: {:?}", reg.bound);

    // Press the combination with a virtual keyboard: hold 800 ms, then release.
    let rt = tokio::runtime::Handle::current();
    let conn = zbus::Connection::session().await.unwrap();
    let inj = mirza_inject::Injector::new(rt, conn).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await; // let the compositor pick up the device
    // KEY_LEFTCTRL 29, KEY_LEFTALT 56, KEY_LEFTSHIFT 42, KEY_F9 67
    let codes = [29u16, 56, 42, 67];
    let t0 = std::time::Instant::now();
    let printer = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            match ev {
                Event::Pressed(id) => println!("{:>5} ms  pressed  {id}", t0.elapsed().as_millis()),
                Event::Released(id) => println!("{:>5} ms  released {id}", t0.elapsed().as_millis()),
                Event::Rebound(id, keys) => println!("rebound {id} to {keys}"),
                Event::Cancelled(id) => println!("cancelled {id}"),
            }
        }
    });
    inj.with_keyboard(|kb| codes.iter().for_each(|&c| kb.set_key(c, true).unwrap()));
    tokio::time::sleep(Duration::from_millis(800)).await;
    inj.with_keyboard(|kb| codes.iter().rev().for_each(|&c| kb.set_key(c, false).unwrap()));
    tokio::time::sleep(Duration::from_secs(1)).await;
    printer.abort();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("this example is for Linux");
}
