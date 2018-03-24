extern crate rix;
extern crate lightify;
extern crate tokio_core;
extern crate futures;
extern crate css_color_parser;
extern crate rscam;
extern crate mime;
extern crate image;

use futures::{Future, Stream};
use css_color_parser::Color;
use lightify::Gateway;
use std::str::FromStr;
use std::io::Read;

fn main() {
    let host = std::env::var("MATRIX_HOST").expect("Missing MATRIX_HOST");
    let token = std::env::var("MATRIX_TOKEN").expect("Missing MATRIX_TOKEN");
    let gateway = std::env::var("GATEWAY_HOST").expect("Missing GATEWAY_HOST");
    let camera_path = std::env::var("CAMERA_PATH").unwrap_or_else(|_| "/dev/video0".to_owned());

    let light_address = {
        let vec: Vec<_> = std::env::var("LIGHT_ADDRESS").expect("Missing LIGHT_ADDRESS")
            .split(":")
            .map(|x| u8::from_str_radix(x, 16).unwrap())
            .collect();
        let mut addr = [0; 8];
        addr.clone_from_slice(&vec);
        addr
    };

    let (color_tx, color_rx) = std::sync::mpsc::channel::<(String, Color)>();
    let (mut image_tx, image_rx) = futures::sync::mpsc::channel::<(String, rscam::Frame)>(2);

    let mut core = tokio_core::reactor::Core::new().unwrap();
    let handle = core.handle();
    let handle2 = core.handle();

    std::thread::spawn(move || {
        let mut camera = rscam::new(&camera_path).unwrap();
        camera.start(&rscam::Config {
            interval: (1, 30),
            format: b"MJPG",
            resolution: (640, 480),
            ..Default::default()
        }).unwrap();

        let mut conn = std::net::TcpStream::connect(gateway).unwrap();

        for (room, color) in color_rx {
            println!("Color: {}", color);
            let r = color.r;
            let g = color.g;
            let b = color.b;
            let w = r.min(g).min(b);

            match conn.set_rgbw(&light_address, &[r, g, b, w]) {
                Ok(_) => {
                    let frame = camera.capture().unwrap();
                    image_tx.try_send((room, frame)).unwrap();
                },
                Err(e) => eprintln!("{:?}", e)
            }
        }
    });

    let task1 = rix::client::sync_stream(
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
                                    Ok(color) => color_tx.send((room.to_owned(), color)).unwrap(),
                                    Err(e) => eprintln!("{:?}", e)
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        })
    .map_err(|e| eprintln!("{:?}", e));

    let task2 = image_rx.for_each(move |(room, frame)| {
        let handle = handle2.clone();
        let room = room.clone();
        let token = token.clone();
        let host = host.clone();
        {
            let frame = image::load_from_memory(&frame[..]).unwrap();
            let mut file = std::fs::File::create("frame.jpg").unwrap();
            frame.save(&mut file, image::JPEG).unwrap();
        }
        let frame = {
            let mut file = std::fs::File::open("frame.jpg").unwrap();
            let mut vec = Vec::new();
            file.read_to_end(&mut vec).unwrap();
            vec
        };
        rix::client::media::upload(&host, &token, &handle2, mime::Mime::from_str("image/jpeg").unwrap(), "frame.jpg", frame)
            .and_then(move |url| {
                rix::client::send_image(&host, &token, &handle, &room, &url, "frame.jpg")
            })
        .map_err(|e| eprintln!("{:?}", e))
    });

    core.run(task1.join(task2)).unwrap();
}
