use std::fs::OpenOptions;
use std::io::Write;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct UiLogger {
    sender: Arc<Mutex<mpsc::Sender<String>>>,
}

impl UiLogger {
    pub fn new(sender: mpsc::Sender<String>) -> Self {
        Self {
            sender: Arc::new(Mutex::new(sender)),
        }
    }

    pub fn log(&self, msg: &str) {
        if let Ok(tx) = self.sender.lock() {
            let formatted = format!("{}\n", msg);
            let _ = tx.send(formatted.clone());

            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open("OVLTool.log")
            {
                let _ = file.write_all(formatted.as_bytes());
            }
        }
    }
}
