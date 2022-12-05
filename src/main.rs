use core::time;
use std::{net::{UdpSocket, SocketAddr}, io, thread::{self, JoinHandle}, sync::{Arc, Mutex}, collections::VecDeque, cell::Cell};
use rand::Rng;

struct UdpProxy {
    proxy: UdpSocket, // Socket that a client from the internet connects to
    proxy_port: u32, // Port number to use when connecting to server
    server: Option<UdpSocket>, // Socket that is connected to a server
    origin: Option<SocketAddr>, // The origin address of the client
}

// THIS FRIGGIN WORKS!!!!!!
fn main() -> io::Result<()>{

    // let args: Vec<_> = env::args().collect();

    // let port_start: u64 = args.get(1).expect("No args specified").parse().expect("Not an integer");
    // let port_end: u64 = args.get(2).expect("No args specified").parse().expect("Not an integer");

    let proxy_conns = Arc::new(Mutex::new({
        let mut x = VecDeque::new();
        for i in 10000..=10100 {
            x.push_back(Cell::new(Some(
               UdpProxy {
                    proxy: {
                        let x = UdpSocket::bind(format!("0.0.0.0:{}", i))?;
                        x.set_nonblocking(true)?;
                        x
                    },
                    server: None,
                    origin: None,
                    proxy_port: i
                }
            )))
        }
        x
    }));

    let size = proxy_conns.lock().unwrap().len();

    let array_ptr = Arc::new(Mutex::new(Cell::new(size)));
    let array_ptr2 = array_ptr.clone();


    let proxy_conns2 = proxy_conns.clone();

    let listen_handler: JoinHandle<io::Result<()>> = thread::spawn(move || {
        let mut buf = [0; 32768];
        loop {
            thread::sleep(time::Duration::from_nanos(rand::thread_rng().gen_range(100..=1000)));
            let cur = {
                let ptr_lock = array_ptr.lock().unwrap();
                let tmp = ptr_lock.take() % size;
                ptr_lock.set(tmp + 1);
                tmp
            };

            // Get a proxy object to use
            let mut chan = {
                let x = proxy_conns.lock().unwrap();
                let y = x.get(cur).unwrap().take();
                if y.is_none() {
                    continue;
                }
                y.unwrap()
            };

            // Get data from client and send to server
            match chan.proxy.recv_from(&mut buf) {
                Ok(n) => {
                    // Get or create a server connection
                    let to_send = match chan.server {
                        Some(t) => t,
                        None => {
                            let x = UdpSocket::bind("0.0.0.0:0")?;
                            x.set_nonblocking(true)?;
                            x.connect(format!("172.17.0.2:{}", chan.proxy_port))?;
                            x
                        }
                    };
                    to_send.send(&buf[..n.0])?;
                    chan.server = Some(to_send);

                    // Save the origin address
                    chan.origin = Some(n.1);
                },
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {

                    // Ensure the server is still listening
                    if chan.server.is_some() {
                        let e = chan.server.as_ref().unwrap();
                        let _ = e.send(&mut [0]);
                    }
                },
                Err(e) => panic!("Eish... encountered IO error: {e}"),
            }
            {
                let x = proxy_conns.lock().unwrap();
                x.get(cur).unwrap().set(Some(chan));
            }
        }
    });

    let server_handler: JoinHandle<io::Result<()>> = thread::spawn(move || {
        let mut buf = [0; 32768];

        loop {
            thread::sleep(time::Duration::from_nanos(rand::thread_rng().gen_range(100..=1000)));
            let cur = {
                let ptr_lock = array_ptr2.lock().unwrap();
                let tmp = ptr_lock.take() % size;
                ptr_lock.set(tmp + 1);
                tmp
            };

            // Get a proxy object to use
            let mut chan = {
                let x = proxy_conns2.lock().unwrap();
                let y = x.get(cur).unwrap().take();
                if y.is_none() {
                    continue;
                }
                y.unwrap()
            };

            let proxy_socket = match chan.server.take() {
                Some(srv) => {

                    // Get data from server. If there is an error, drop the server connection
                    let is_connected = match srv.recv_from(&mut buf) {
                        Ok(n) => {
                            chan.proxy.send_to(&buf[..n.0], chan.origin.unwrap())?;
                            true
                        },
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => true,
                        Err(e) => {
                            false
                        },
                    };
                    if is_connected {
                        Some(srv)
                    } else {
                        None
                    }
                },
                None => None
            };

            chan.server = proxy_socket;
            {
                let x = proxy_conns2.lock().unwrap();
                x.get(cur).unwrap().set(Some(chan));
            }
        }
    });

    listen_handler.join().unwrap()?;
    server_handler.join().unwrap()?;

    Ok(())
}
