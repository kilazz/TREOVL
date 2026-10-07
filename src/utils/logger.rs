use parking_lot::Mutex;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc;

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
        let formatted = format!("{}\n", msg);
        let tx = self.sender.lock();
        let _ = tx.send(formatted.clone());

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("TREOVL.log")
        {
            let _ = file.write_all(formatted.as_bytes());
        }
    }
}
