extern crate rix;
extern crate lightify;
extern crate tokio_core;
extern crate futures;
extern crate css_color_parser;

use futures::{Future, Stream};
use css_color_parser::Color;
use lightify::Gateway;

fn main() {
    let host = std::env::var("MATRIX_HOST").expect("Missing MATRIX_HOST");
    let token = std::env::var("MATRIX_TOKEN").expect("Missing MATRIX_TOKEN");
    let gateway = std::env::var("GATEWAY_HOST").expect("Missing GATEWAY_HOST");

    let light_address = {
        let light_address_vec = std::env::var("LIGHT_ADDRESS").expect("Missing LIGHT_ADDRESS")
            .split(":")
            .map(|x| x.parse().unwrap())
            .collect::<Vec<u8>>();
        let mut addr = [0; 8];
        addr.clone_from_slice(&light_address_vec);
        addr
    };

    let (tx, rx) = std::sync::mpsc::channel::<Color>();

    std::thread::spawn(move || {
        let mut conn = std::net::TcpStream::connect(gateway).unwrap();

        for color in rx {
            println!("Color: {}", color);
            let r = color.r;
            let g = color.g;
            let b = color.b;
            let w = r.min(g).min(b);

            match conn.set_rgbw(&light_address, &[r, g, b, w]) {
                Ok(_) => {},
                Err(e) => eprintln!("{:?}", e)
            }
        }
    });

    let mut core = tokio_core::reactor::Core::new().unwrap();
    let handle = core.handle();

    let task = rix::client::sync_stream(
        &host,
        &token,
        &handle)
        .skip(1)
        .for_each(|frame| {
            for evt in frame.events() {
                if evt.event_type == "m.room.message" {
                    if let Some(body) = evt.content["body"].as_str() {
                        if let Some(ref room) = evt.room {
                            const PREFIX: &str = "%color";
                            println!("Message: {}", body);
                            if body.starts_with(PREFIX) {
                                let request = body[PREFIX.len()..].trim();
                                match request.parse::<Color>() {
                                    Ok(color) => tx.send(color).unwrap(),
                                    Err(e) => eprintln!("{:?}", e)
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        });

    core.run(task).unwrap();
}
