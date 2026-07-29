use chess_engine::play;
use chess_engine::search::bench;
use chess_engine::server;
use chess_engine::uci::{ENGINE_NAME, Uci};

fn main() {
    chess_engine::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // `play [white|black] [movetime_ms]` — interactive game at a terminal.
        Some("play") => play::start(&args[1..]),
        // `serve [port] [white|black] [movetime_ms]` — play in a browser.
        Some("serve") => server::start(&args[1..]),
        // `bench [depth]` gives a reproducible node count for regression checks.
        Some("bench") => {
            let depth = args.get(1).and_then(|d| d.parse().ok()).unwrap_or(8);
            bench(depth);
        }
        _ => {
            println!("{ENGINE_NAME} — UCI chess engine.");
            println!("Type 'uci' for the protocol, or restart with 'play' to play a game.");
            Uci::new().run();
        }
    }
}
