//! A tiny HTTP server so you can play the engine in a browser.
//!
//! Deliberately minimal: enough HTTP/1.1 to serve three static files and one
//! JSON endpoint, using only `std::net`. The engine runs natively here, at
//! full speed — the page is only a view onto a `play::Game`.
//!
//! Binds to loopback only. This is a local single-player app, not a service:
//! there is no authentication and one shared game per process.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::play::{EngineMove, Game};
use crate::types::Color;

// The React front-end, built by `npm run build` in web/ into src/web/dist and
// embedded here so the binary is self-contained and `cargo build` needs no
// Node. See web/README.md for the development loop.
const INDEX_HTML: &str = include_str!("web/dist/index.html");
const STYLE_CSS: &str = include_str!("web/dist/style.css");
const APP_JS: &str = include_str!("web/dist/app.js");

/// Requests that take longer than this to arrive are dropped, so a browser
/// pre-opening a connection cannot tie up a thread.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY: usize = 8 * 1024;

pub struct Server {
    listener: TcpListener,
    game: Arc<Mutex<Game>>,
}

impl Server {
    pub fn bind(
        address: impl ToSocketAddrs,
        human: Color,
        movetime: u64,
    ) -> std::io::Result<Server> {
        let listener = TcpListener::bind(address)?;
        let mut game = Game::new(human);
        game.set_movetime(movetime);
        Ok(Server {
            listener,
            game: Arc::new(Mutex::new(game)),
        })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves until the process is killed. One thread per connection; the game
    /// itself is behind a mutex, so requests during a search simply wait.
    pub fn run(&self) {
        for stream in self.listener.incoming() {
            let Ok(stream) = stream else { continue };
            let game = Arc::clone(&self.game);
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                if let Err(err) = handle_connection(stream, &game) {
                    // A browser closing a tab mid-request is routine.
                    if err.kind() != std::io::ErrorKind::BrokenPipe {
                        eprintln!("connection error: {err}");
                    }
                }
            });
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// Reads one HTTP request. Returns `None` if the stream closed or the request
/// line was malformed.
pub fn parse_request(reader: &mut impl BufRead) -> std::io::Result<Option<Request>> {
    let mut start_line = String::new();
    if reader.read_line(&mut start_line)? == 0 {
        return Ok(None);
    }
    let mut parts = start_line.split_whitespace();
    let (Some(method), Some(path)) = (parts.next(), parts.next()) else {
        return Ok(None);
    };
    let (method, path) = (method.to_string(), path.to_string());

    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }

    let mut body = vec![0u8; content_length.min(MAX_BODY)];
    if !body.is_empty() {
        reader.read_exact(&mut body)?;
    }
    Ok(Some(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
    }))
}

fn handle_connection(stream: TcpStream, game: &Mutex<Game>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    let Some(request) = parse_request(&mut reader)? else {
        return Ok(());
    };
    // Strip any query string; this app doesn't use one.
    let path = request.path.split('?').next().unwrap_or("/");

    let (status, content_type, body) = match (request.method.as_str(), path) {
        ("GET", "/") => ("200 OK", "text/html; charset=utf-8", INDEX_HTML.to_string()),
        ("GET", "/style.css") => ("200 OK", "text/css; charset=utf-8", STYLE_CSS.to_string()),
        ("GET", "/app.js") => (
            "200 OK",
            "text/javascript; charset=utf-8",
            APP_JS.to_string(),
        ),
        ("POST", "/api") => {
            let mut game = game.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                "200 OK",
                "application/json; charset=utf-8",
                run_command(&mut game, request.body.trim()),
            )
        }
        ("GET", _) | ("POST", _) => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found".into(),
        ),
        _ => (
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            "method not allowed".into(),
        ),
    };

    write!(
        writer,
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )?;
    writer.flush()
}

/// Runs one command from the page and returns the new game state as JSON.
///
/// The human's move and the engine's reply are separate commands so the page
/// can paint the human's move immediately and show "thinking" while it waits.
pub fn run_command(game: &mut Game, command: &str) -> String {
    let (verb, rest) = command.split_once(' ').unwrap_or((command, ""));
    let rest = rest.trim();
    let mut message = String::new();
    let mut engine_move = None;

    match verb {
        "state" | "" => {}
        "move" => match game.play_move(rest) {
            Ok(san) => message = format!("You played {san}"),
            Err(why) => message = why,
        },
        "engine" => {
            if game.is_engines_turn() {
                engine_move = game.engine_reply();
            }
        }
        "hint" => match game.suggestion() {
            Some(best) => message = format!("Try {} ({})", best.san, best.score),
            None => message = "no suggestion available".into(),
        },
        "undo" => match game.take_back() {
            0 => message = "nothing to undo".into(),
            n => message = format!("took back {n} half-move(s)"),
        },
        "resign" => game.resign(),
        "new" => {
            let mut human = game.human();
            let mut movetime = game.movetime();
            for argument in rest.split_whitespace() {
                match argument {
                    "white" | "w" => human = Color::White,
                    "black" | "b" => human = Color::Black,
                    other => {
                        if let Ok(ms) = other.parse::<u64>() {
                            movetime = ms;
                        }
                    }
                }
            }
            *game = Game::new(human);
            game.set_movetime(movetime);
        }
        "time" => {
            if let Ok(ms) = rest.parse::<u64>() {
                game.set_movetime(ms);
            }
        }
        other => message = format!("unknown command '{other}'"),
    }

    state_json(game, &message, engine_move.as_ref())
}

fn state_json(game: &Game, message: &str, engine_move: Option<&EngineMove>) -> String {
    let outcome = game.outcome();
    let engine = match engine_move {
        Some(mv) => format!(
            r#"{{"san":"{}","uci":"{}","score":"{}","scoreCp":{},"depth":{},"nodes":{},"millis":{}}}"#,
            escape(&mv.san),
            escape(&mv.uci),
            escape(&mv.score),
            mv.score_cp,
            mv.depth,
            mv.nodes,
            mv.millis
        ),
        None => "null".to_string(),
    };
    let legal: Vec<String> = game
        .legal_uci()
        .iter()
        .map(|m| format!("\"{m}\""))
        .collect();
    let history: Vec<String> = game
        .history_san()
        .iter()
        .map(|san| format!("\"{}\"", escape(san)))
        .collect();

    format!(
        r#"{{"fen":"{fen}","turn":"{turn}","human":"{human}","check":{check},"over":{over},"result":"{result}","engineToMove":{engine_to_move},"lastMove":"{last}","materialBalance":{balance},"movetime":{movetime},"message":"{message}","engine":{engine},"legal":[{legal}],"history":[{history}]}}"#,
        fen = escape(&game.fen()),
        turn = color_name(game.side_to_move()),
        human = color_name(game.human()),
        check = game.in_check(),
        over = outcome.is_some(),
        result = outcome
            .map(|o| escape(&o.describe(game.human())))
            .unwrap_or_default(),
        engine_to_move = outcome.is_none() && game.is_engines_turn(),
        last = game
            .last_move()
            .map(|mv| mv.to_string())
            .unwrap_or_default(),
        balance = game.material_balance(),
        movetime = game.movetime(),
        message = escape(message),
        legal = legal.join(","),
        history = history.join(","),
    )
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "white",
        Color::Black => "black",
    }
}

/// Escapes the few characters that can appear in our strings and would break
/// the JSON. Everything here is engine-generated ASCII plus the em dash.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Parses `serve [port] [white|black] [movetime_ms]`. Numbers are positional —
/// the first is the port and the second the thinking time — because the two
/// ranges overlap and guessing from magnitude gets it wrong.
pub fn parse_args(args: &[String]) -> (u16, Color, u64) {
    let mut port = 8080u16;
    let mut human = Color::White;
    let mut movetime = 1000u64;
    let mut numbers = 0;

    for argument in args {
        match argument.as_str() {
            "white" | "w" => human = Color::White,
            "black" | "b" => human = Color::Black,
            other => match other.parse::<u64>() {
                Ok(value) => {
                    numbers += 1;
                    match numbers {
                        1 => port = value.min(65535) as u16,
                        2 => movetime = value.clamp(10, 60_000),
                        _ => eprintln!("ignoring extra number '{other}'"),
                    }
                }
                Err(_) => eprintln!("ignoring unknown argument '{other}'"),
            },
        }
    }
    (port, human, movetime)
}

/// Starts serving, or explains why it could not.
pub fn start(args: &[String]) {
    let (port, human, movetime) = parse_args(args);
    match Server::bind(("127.0.0.1", port), human, movetime) {
        Ok(server) => {
            let address = server.local_addr().expect("bound socket has an address");
            println!("Playing at http://{address}  (Ctrl-C to stop)");
            server.run();
        }
        Err(err) => {
            eprintln!("could not bind port {port}: {err}");
            eprintln!("try another one, e.g. `chess-engine serve 8090`");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    fn request_from(raw: &str) -> Option<Request> {
        parse_request(&mut BufReader::new(raw.as_bytes())).unwrap()
    }

    #[test]
    fn parses_a_get() {
        let request = request_from("GET /app.js HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/app.js");
        assert!(request.body.is_empty());
    }

    #[test]
    fn parses_a_post_body() {
        let raw = "POST /api HTTP/1.1\r\nContent-Length: 7\r\n\r\nmove e4";
        let request = request_from(raw).unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.body, "move e4");
    }

    #[test]
    fn handles_broken_input() {
        assert!(request_from("").is_none());
        assert!(request_from("nonsense\r\n\r\n").is_none());
        // A body shorter than Content-Length claims must not hang forever.
        assert!(
            parse_request(&mut BufReader::new(
                "POST /api HTTP/1.1\r\nContent-Length: 99\r\n\r\nshort".as_bytes()
            ))
            .is_err()
        );
    }

    #[test]
    fn state_command_reports_the_start_position() {
        crate::init();
        let mut game = Game::new(Color::White);
        let json = run_command(&mut game, "state");
        assert!(json.contains(r#""turn":"white""#));
        assert!(json.contains(r#""over":false"#));
        assert!(json.contains(r#""engineToMove":false"#));
        assert!(json.contains(r#""e2e4""#), "legal moves should be listed");
        assert!(json.contains(r#""engine":null"#));
    }

    #[test]
    fn move_then_engine_are_separate_steps() {
        crate::init();
        let mut game = Game::new(Color::White);
        game.set_movetime(50);

        let json = run_command(&mut game, "move e4");
        assert!(json.contains(r#""engineToMove":true"#), "engine's turn now");
        assert!(
            json.contains(r#""engine":null"#),
            "engine has not moved yet"
        );
        assert!(json.contains(r#""history":["e4"]"#));

        let json = run_command(&mut game, "engine");
        assert!(json.contains(r#""engineToMove":false"#));
        assert!(
            !json.contains(r#""engine":null"#),
            "engine move should be reported"
        );
        assert!(json.contains(r#""depth""#));
    }

    #[test]
    fn illegal_moves_come_back_as_a_message() {
        crate::init();
        let mut game = Game::new(Color::White);
        let json = run_command(&mut game, "move e5");
        assert!(json.contains("is not a legal move"), "{json}");
        assert!(json.contains(r#""history":[]"#));
    }

    #[test]
    fn undo_new_and_time_commands_work() {
        crate::init();
        let mut game = Game::new(Color::White);
        game.set_movetime(50);
        run_command(&mut game, "move e4");
        run_command(&mut game, "engine");

        let json = run_command(&mut game, "undo");
        assert!(json.contains(r#""history":[]"#), "{json}");

        let json = run_command(&mut game, "new black 250");
        assert!(json.contains(r#""human":"black""#));
        assert!(json.contains(r#""engineToMove":true"#));
        assert!(json.contains(r#""movetime":250"#));

        let json = run_command(&mut game, "time 1500");
        assert!(json.contains(r#""movetime":1500"#));
    }

    #[test]
    fn resignation_ends_the_game() {
        crate::init();
        let mut game = Game::new(Color::White);
        let json = run_command(&mut game, "resign");
        assert!(json.contains(r#""over":true"#));
        assert!(json.contains("Resignation"));
    }

    #[test]
    fn command_line_numbers_are_positional() {
        let args =
            |text: &str| -> Vec<String> { text.split_whitespace().map(str::to_string).collect() };
        assert_eq!(parse_args(&[]), (8080, Color::White, 1000));
        assert_eq!(parse_args(&args("9000")), (9000, Color::White, 1000));
        // The second number is a thinking time, not another port.
        assert_eq!(
            parse_args(&args("8099 white 300")),
            (8099, Color::White, 300)
        );
        assert_eq!(parse_args(&args("black")), (8080, Color::Black, 1000));
        assert_eq!(parse_args(&args("8090 b 2500")), (8090, Color::Black, 2500));
        // Junk is ignored rather than fatal.
        assert_eq!(parse_args(&args("nonsense")), (8080, Color::White, 1000));
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(escape("line\nbreak"), "line\\nbreak");
        // The result messages contain an em dash, which is valid raw JSON.
        assert_eq!(escape("Checkmate — you win!"), "Checkmate — you win!");
    }

    /// Starts a real server on an ephemeral port and talks HTTP to it.
    #[test]
    fn serves_over_a_real_socket() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpStream;

        crate::init();
        let server = Server::bind(("127.0.0.1", 0), Color::White, 50).unwrap();
        let address = server.local_addr().unwrap();
        std::thread::spawn(move || server.run());

        let fetch = |request: &str| {
            let mut stream = TcpStream::connect(address).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        };

        let page = fetch("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(page.starts_with("HTTP/1.1 200 OK"));
        assert!(
            page.contains("<div id=\"root\""),
            "index.html should be served"
        );
        assert!(page.contains("/app.js"), "page should load the bundle");

        let js = fetch("GET /app.js HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(js.contains("text/javascript"));
        let css = fetch("GET /style.css HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(css.contains("text/css"));

        let api = fetch("POST /api HTTP/1.1\r\nContent-Length: 7\r\n\r\nmove e4");
        assert!(api.contains(r#""history":["e4"]"#), "{api}");

        let missing = fetch("GET /nope HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(missing.starts_with("HTTP/1.1 404"));
    }
}
