//! How long `AppState::clone` takes, alone and on two threads at once.
use renox::prelude::*;
use std::time::Instant;

#[tokio::main]
async fn main() {
    let app = renox::testing::TestApp::new(App::new()).await;
    let state = app.state().clone();
    const N: u32 = 1_000_000;
    let t = Instant::now();
    for _ in 0..N {
        std::hint::black_box(state.clone());
    }
    println!(
        "one thread: {:.0} ns per clone",
        t.elapsed().as_nanos() as f64 / N as f64
    );
    let t = Instant::now();
    let a = state.clone();
    let b = state.clone();
    let h1 = std::thread::spawn(move || {
        for _ in 0..N {
            std::hint::black_box(a.clone());
        }
    });
    let h2 = std::thread::spawn(move || {
        for _ in 0..N {
            std::hint::black_box(b.clone());
        }
    });
    h1.join().unwrap();
    h2.join().unwrap();
    println!(
        "two threads: {:.0} ns per clone (wall time / N)",
        t.elapsed().as_nanos() as f64 / N as f64
    );
}
