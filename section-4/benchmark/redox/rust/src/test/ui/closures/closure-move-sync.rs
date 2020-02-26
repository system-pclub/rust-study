// ignore-x86 FIXME: missing sysroot spans (#53081)
use std::thread;
use std::sync::mpsc::channel;

fn bar() {
    let (send, recv) = channel();
    let t = thread::spawn(|| {
        recv.recv().unwrap();
        //~^^ ERROR `std::sync::mpsc::Receiver<()>` cannot be shared between threads safely
    });

    send.send(());

    t.join().unwrap();
}

fn foo() {
    let (tx, _rx) = channel();
    thread::spawn(|| tx.send(()).unwrap());
    //~^ ERROR `std::sync::mpsc::Sender<()>` cannot be shared between threads safely
}

fn main() {}
