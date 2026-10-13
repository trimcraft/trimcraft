//! "Play sound file" actions: decodes wav, mp3, ogg and flac (rodio) and plays them on the
//! default or a chosen output device, on a thread of its own.
//!
//! Several sounds can play at once; "stop previous" stops the ones still playing first. The
//! output streams are opened when needed and closed once nothing plays, so a changed default
//! device is picked up by the next sound.

use crate::executor::Done;
use rodio::cpal::traits::HostTrait;
use rodio::{Decoder, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Player};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

/// How often playing sounds are checked for having ended.
const POLL: Duration = Duration::from_millis(50);

pub struct Play {
    pub path: String,
    /// 0..=100.
    pub volume: u8,
    /// `None`: the default device.
    pub device: Option<String>,
    pub stop_previous: bool,
}

enum Job {
    Play(Play, Done),
    Devices(Sender<Result<Vec<String>, String>>),
}

pub struct Sound {
    queue: Sender<Job>,
}

impl Sound {
    pub fn spawn() -> Self {
        let (queue, jobs) = channel();
        std::thread::Builder::new()
            .name("trimcraft-sound".into())
            .spawn(move || run(jobs))
            .expect("spawn sound thread");
        Self { queue }
    }

    pub fn play(&self, play: Play, done: Done) {
        if let Err(e) = self.queue.send(Job::Play(play, done)) {
            let Job::Play(_, done) = e.0 else { return };
            done(Err("the sound thread is gone".into()));
        }
    }

    /// Names of the output devices, for the picker.
    pub fn devices(&self) -> Result<Vec<String>, String> {
        let (reply, answer) = channel();
        self.queue
            .send(Job::Devices(reply))
            .map_err(|_| "the sound thread is gone".to_owned())?;
        answer
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "the sound thread did not answer".to_owned())?
    }
}

struct Playing {
    player: Player,
    done: Option<Done>,
}

fn run(jobs: Receiver<Job>) {
    // Output streams by device name (`None`: default), while something plays on them.
    let mut sinks: HashMap<Option<String>, MixerDeviceSink> = HashMap::new();
    let mut playing: Vec<Playing> = Vec::new();
    loop {
        let job = if playing.is_empty() {
            sinks.clear();
            match jobs.recv() {
                Ok(job) => Some(job),
                Err(_) => break,
            }
        } else {
            match jobs.recv_timeout(POLL) {
                Ok(job) => Some(job),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };
        match job {
            None => {}
            Some(Job::Devices(reply)) => {
                let _ = reply.send(device_names());
            }
            Some(Job::Play(play, done)) => {
                if play.stop_previous {
                    for p in playing.drain(..) {
                        p.player.stop();
                        if let Some(done) = p.done {
                            done(Ok("Stopped by the next sound".into()));
                        }
                    }
                }
                match start(&mut sinks, &play) {
                    Ok(player) => playing.push(Playing {
                        player,
                        done: Some(done),
                    }),
                    Err(e) => done(Err(e)),
                }
            }
        }
        playing.retain_mut(|p| {
            if !p.player.empty() {
                return true;
            }
            if let Some(done) = p.done.take() {
                done(Ok("Played".into()));
            }
            false
        });
    }
}

fn start(
    sinks: &mut HashMap<Option<String>, MixerDeviceSink>,
    play: &Play,
) -> Result<Player, String> {
    let file =
        std::fs::File::open(&play.path).map_err(|e| format!("cannot open {}: {e}", play.path))?;
    let source = Decoder::try_from(file)
        .map_err(|e| format!("cannot play {} ({e}); use wav, mp3, ogg or flac", play.path))?;
    let key = play.device.clone();
    if !sinks.contains_key(&key) {
        let mut sink = open(play.device.as_deref())?;
        sink.log_on_drop(false);
        sinks.insert(key.clone(), sink);
    }
    let sink = &sinks[&key];
    let player = Player::connect_new(sink.mixer());
    player.set_volume(f32::from(play.volume.min(100)) / 100.0);
    player.append(source);
    Ok(player)
}

fn open(device: Option<&str>) -> Result<MixerDeviceSink, String> {
    let Some(name) = device else {
        return DeviceSinkBuilder::open_default_sink().map_err(|e| format!("no sound output: {e}"));
    };
    let host = rodio::cpal::default_host();
    let found = host
        .output_devices()
        .map_err(|e| format!("cannot list sound outputs: {e}"))?
        .find(|d| d.description().is_ok_and(|desc| desc.name() == name))
        .ok_or_else(|| format!("sound output \"{name}\" not found"))?;
    DeviceSinkBuilder::from_device(found)
        .and_then(DeviceSinkBuilder::open_stream)
        .map_err(|e| format!("cannot open sound output \"{name}\": {e}"))
}

fn device_names() -> Result<Vec<String>, String> {
    let host = rodio::cpal::default_host();
    let mut names: Vec<String> = host
        .output_devices()
        .map_err(|e| format!("cannot list sound outputs: {e}"))?
        .filter_map(|d| d.description().ok().map(|desc| desc.name().to_owned()))
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    /// Lists the output devices; plays nothing.
    #[test]
    #[ignore = "needs sound hardware"]
    fn lists_output_devices() {
        let devices = super::Sound::spawn().devices().unwrap();
        println!("{devices:#?}");
        assert!(!devices.is_empty());
    }
}
