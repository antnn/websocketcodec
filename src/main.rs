#![feature(async_await)]
use {
    hyper::{StatusCode, server::conn::Connection, Body, Client, Request, Response, Server, Uri,
            header::{ UPGRADE, CONTENT_LENGTH,
                      CONNECTION,
                      SEC_WEBSOCKET_VERSION,
                      SEC_WEBSOCKET_KEY,
                      SEC_WEBSOCKET_ACCEPT,
                      SEC_WEBSOCKET_PROTOCOL, HeaderValue},
            service::{make_service_fn, service_fn},
            upgrade::Upgraded,
            server::conn::{Http, Connecting},

    },
    std:: {str, net::SocketAddr},
    futures:: { stream::Stream },
    futures_util::TryStreamExt,
    tokio::{ sync::oneshot, io::{AsyncReadExt, AsyncWriteExt} },
    tokio_tcp::{TcpListener, TcpStream},
    sha1::Sha1
};
use std::convert::TryInto;



//use futures;



// Note: `hyper::upgrade` docs link to this upgrade.






// A simple type alias so as to DRY.
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Handle server-side I/O after HTTP upgraded.
///


async fn server_upgraded_io(mut upgraded: Upgraded) -> Result<()> {

    let mut v = vec![0u8; 136];
    let len = upgraded.read(&mut v).await?;

    if len < 2 {
        ()
    }

    let final_bit = 0x80u8;
    let kReserved1Bit = 0x40u8;
    let kReserved2Bit = 0x20u8;
    let kReserved3Bit = 0x10u8;
    let kOpCodeMask = 0xFu8;
    let kMaskBit = 0x80u8;
    let kPayloadLengthMask = 0x7Fu8;
    let kMaxPayloadLengthWithoutExtendedLengthField = 125u64;
    let kPayloadLengthWithTwoByteExtendedLengthField = 126u64;
    let kPayloadLengthWithEightByteExtendedLengthField = 127u64;

    let first_byte = v[0];
    let second_byte = v[1];

    let fin    = (first_byte & final_bit) != 0;
    let reserved1 = (first_byte & kReserved1Bit) != 0;
    let reserved2 = (first_byte & kReserved2Bit) != 0;
    let reserved3 = (first_byte & kReserved3Bit) != 0;
    let opcode  =  first_byte & kOpCodeMask;
    let masked = (second_byte & kMaskBit) != 0;
    let mut payload_length = (second_byte & kPayloadLengthMask) as u64;

    let mut mask_offset=2usize;
    if payload_length == 126 {
        if len < 4 { () };
        mask_offset += 2;
        let a: [u8;2] = v[2..4].try_into().unwrap();
        payload_length = u16::from_be_bytes(a) as u64;
    } else if payload_length == 127  {
        if len < 10 { () };
        mask_offset += 8;
        let a: [u8;8] = v[2..10].try_into().unwrap();
        payload_length = u64::from_le_bytes(a) as u64;
    }

    if (len as u64) < (payload_length + 4 /*mask size*/ + mask_offset as u64 + 2 /*header*/)  { () }

    let mut mask= 0u64;
    if masked {
        let mask_slice = &v[mask_offset..(mask_offset+4)];
        let mut mask_bytes = [0u8; 4];
        mask_bytes.copy_from_slice(mask_slice);
        mask = u32::from_le_bytes(mask_bytes) as u64;
        mask = (mask << 32) + mask; //to speedup things "convert" to double word
    };
    let chunks = (payload_length as f64 / 8.0).ceil() as usize;
    let mut vec64 = unsafe {
        let mut ptr = v.as_mut_ptr() as *mut u8;
        ptr = ptr.offset( (mask_offset+4) as isize);
        std::mem::forget(v);
        let p64 = ptr as *mut u64;
        Vec::from_raw_parts(p64, chunks, chunks) 
    };

    let mut i = 0usize;
    for i in  0..chunks {
        vec64[i] = vec64[i] ^ mask; // unmasking double world chunks obviously faster
    }
    let remainder = payload_length % 8;
    if remainder != 0 {
        *vec64.last_mut().unwrap() = *vec64.last().unwrap() << (64-remainder*8);
    }
    let mut vec8 = unsafe {
        let ratio = std::mem::size_of::<u64>() / std::mem::size_of::<u8>();
        let length = payload_length  as usize;
        let capacity = payload_length as usize;
        let ptr = vec64.as_mut_ptr() as *mut u8;
        std::mem::forget(vec64);
        Vec::from_raw_parts(ptr, length, capacity)
    };


    println!("{:?}",str::from_utf8(&vec8));





    //upgraded.write_all(b"barr=foo").await?;
    //println!("server[foobar] sent");

    //println!("{:?}", upgraded);
    loop {}
    Ok(())
}

/// Our server HTTP handler to initiate HTTP upgrades.
async fn server_upgrade(req: Request<Body>) -> Result<Response<Body>> {
    let mut b = Body::empty();
    let mut res = Response::new(b);

    // Send a 400 to any request that doesn't have
    // an `Upgrade` header.

    let mut key = req.headers().get(SEC_WEBSOCKET_KEY).unwrap().to_str().unwrap();
    let accept = String::new()+ key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    "1.6.  Security Model";
    if !check_request(&req){
        *res.status_mut() = StatusCode::BAD_REQUEST;
        res.headers_mut().insert(SEC_WEBSOCKET_VERSION,
                                 HeaderValue::from_static("13"));
        return Ok(res);
    }
    // Setup a future that will eventually receive the upgraded
    // connection and talk a new protocol, and spawn the future
    // into the runtime.
    //
    // Note: This can't possibly be fulfilled until the 101 response
    // is returned below, so it's better to spawn this future instead
    // waiting for it to complete to then return a response.
    hyper::rt::spawn(async move {
        match req.into_body().on_upgrade().await {
            Ok(upgraded) => {
                if let Err(e) = server_upgraded_io(upgraded).await {
                    eprintln!("server foobar io error: {}", e)
                };
            }
            Err(e) => eprintln!("upgrade error: {}", e)
        }
    });


    *res.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    res.headers_mut().insert(UPGRADE, HeaderValue::from_static("websocket"));
    res.headers_mut().insert(CONNECTION, HeaderValue::from_static("Upgrade"));

    let mut sha = sha1::Sha1::new();
    sha.update(accept.as_bytes());
    let bytes = sha.digest().bytes();

    let accept = base64::encode(&bytes);

    res.headers_mut().insert(SEC_WEBSOCKET_ACCEPT,
                             HeaderValue::from_str(accept.as_str()).unwrap());
    res.headers_mut().insert(SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_static("rust-websocket"));


    Ok(res)
}

/// Handle client-side I/O after HTTP upgraded.
async fn client_upgraded_io(mut upgraded: Upgraded) -> Result<()> {
    // We've gotten an upgraded connection that we can read
    // and write directly on. Let's start out 'foobar' protocol.
    upgraded.write_all(b"foo=bar").await?;
    println!("client[foobar] sent");

    let mut vec = Vec::new();
    upgraded.read_to_end(&mut vec).await?;
    println!("client[foobar] recv: {:?}", str::from_utf8(&vec));

    Ok(())
}



#[hyper::rt::main]
async fn main() {
    // For this example, we just make a server and our own client to talk to
    // it, so the exact port isn't important. Instead, let the OS give us an
    // unused port.
    let addr = ([127, 0, 0, 1], 3000).into();

    let make_service = make_service_fn(|_| async {
        Ok::<_, hyper::Error>(service_fn(server_upgrade))
    });

    let server = Server::bind(&addr)
        .serve(make_service);

    // We need the assigned address for the client to send it messages.
    let addr = server.local_addr();

    // For this example, a oneshot is used to signal that after 1 request,
    // the server should be shutdown.
    let (tx, rx) = oneshot::channel::<()>();
    let server = server
        .with_graceful_shutdown(async {
            rx.await.ok();
        });

    // Spawn server on the default executor,
    // which is usually a thread-pool from tokio default runtime.
    hyper::rt::spawn(async {
        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
    });

    // Client requests a HTTP connection upgrade.

    loop {}

    // Complete the oneshot so that the server stops
    // listening and the process can close down.
    let _ = tx.send(());
}



fn check_request(req: &Request<Body>) -> bool{
    let ver = req.headers().get(SEC_WEBSOCKET_VERSION).unwrap().to_str().unwrap();
    req.headers().contains_key(UPGRADE) ||
        req.headers().contains_key(CONNECTION) ||
        req.headers().contains_key(SEC_WEBSOCKET_KEY) ||
        ver == "13"
 }


