use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const MAX_RECORDING_SECONDS: u64 = 300;
// Leave room for the WAV header below the transcription service's 25 MB limit.
pub const MAX_AUDIO_FILE_BYTES: u64 = 24_000_000;
const MAX_AUDIO_SAMPLES: usize = (MAX_AUDIO_FILE_BYTES as usize - 44) / 2;

pub type AudioLevelCallback = Arc<dyn Fn(f32) + Send + Sync>;
pub type RecordingEventCallback = Arc<dyn Fn(RecordingEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum RecordingEventKind {
    LimitReached,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct RecordingEvent {
    pub session_id: String,
    pub kind: RecordingEventKind,
}

impl RecordingEvent {
    pub fn belongs_to(&self, current_session: Option<&str>) -> bool {
        current_session == Some(self.session_id.as_str())
    }
}

pub struct AudioRecorder {
    recording: Mutex<Option<Recording>>,
    worker_running: Arc<AtomicBool>,
    audio_level_callback: Arc<Mutex<Option<AudioLevelCallback>>>,
    recording_event_callback: Arc<Mutex<Option<RecordingEventCallback>>>,
}

struct Recording {
    capture: Arc<Capture>,
    started_at: Instant,
    worker: thread::JoinHandle<()>,
}

struct Capture {
    active: AtomicBool,
    samples: Mutex<Vec<i16>>,
    sample_rate: AtomicU32,
    limit_reached: AtomicBool,
    failure: Mutex<Option<String>>,
}

impl Capture {
    fn new() -> Self {
        Self {
            active: AtomicBool::new(true),
            samples: Mutex::new(Vec::new()),
            sample_rate: AtomicU32::new(44_100),
            limit_reached: AtomicBool::new(false),
            failure: Mutex::new(None),
        }
    }
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            recording: Mutex::new(None),
            worker_running: Arc::new(AtomicBool::new(false)),
            audio_level_callback: Arc::new(Mutex::new(None)),
            recording_event_callback: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_audio_level_callback(&self, callback: Option<AudioLevelCallback>) {
        *self.audio_level_callback.lock().unwrap() = callback;
    }

    pub fn set_recording_event_callback(&self, callback: Option<RecordingEventCallback>) {
        *self.recording_event_callback.lock().unwrap() = callback;
    }

    pub fn start(&self, session_id: String) -> anyhow::Result<()> {
        let mut recording = self.recording.lock().unwrap();
        if recording.is_some() {
            return Ok(());
        }

        let level_callback = self.audio_level_callback.clone();
        let next = start_capture_worker(
            self.worker_running.clone(),
            session_id,
            self.recording_event_callback.clone(),
            Duration::from_secs(10),
            move |capture, ready_sender, ready| {
                record_loop(capture, level_callback, ready_sender, ready)
            },
        )?;
        *recording = Some(next);
        Ok(())
    }

    pub fn cancel(&self) {
        let recording = self.recording.lock().unwrap().take();
        if let Some(recording) = recording {
            recording.capture.active.store(false, Ordering::SeqCst);
            let _ = recording.worker.join();
        }
    }

    pub fn stop_to_wav(&self) -> anyhow::Result<Option<tempfile::TempPath>> {
        let Some(recording) = self.recording.lock().unwrap().take() else {
            return Ok(None);
        };
        recording.capture.active.store(false, Ordering::SeqCst);
        recording
            .worker
            .join()
            .map_err(|_| anyhow::anyhow!("Microphone worker stopped unexpectedly"))?;
        if let Some(failure) = recording.capture.failure.lock().unwrap().take() {
            anyhow::bail!(failure);
        }

        if recording.started_at.elapsed() < Duration::from_millis(350) {
            return Ok(None);
        }

        // Move samples out rather than retaining and cloning a complete recording.
        let samples = std::mem::take(&mut *recording.capture.samples.lock().unwrap());
        if samples.len() < 1_000 {
            return Ok(None);
        }
        let sample_rate = recording.capture.sample_rate.load(Ordering::SeqCst);
        write_wav(&samples, sample_rate).map(Some)
    }
}

struct CaptureWorkerGuard(Arc<AtomicBool>);

impl Drop for CaptureWorkerGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn start_capture_worker(
    worker_running: Arc<AtomicBool>,
    session_id: String,
    event_callback: Arc<Mutex<Option<RecordingEventCallback>>>,
    timeout: Duration,
    work: impl FnOnce(Arc<Capture>, &mpsc::SyncSender<Result<(), String>>, &mut bool) -> anyhow::Result<()>
        + Send
        + 'static,
) -> anyhow::Result<Recording> {
    worker_running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| anyhow::anyhow!("The previous microphone worker is still closing. Check your device or restart Vellora before trying again."))?;
    let guard = CaptureWorkerGuard(worker_running);
    let capture = Arc::new(Capture::new());
    let worker_capture = capture.clone();
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        // A timeout cannot stop a blocked Windows driver. Keep this guard until
        // the worker actually exits so repeated retries cannot accumulate threads.
        let _guard = guard;
        let mut ready = false;
        let result = work(worker_capture.clone(), &ready_sender, &mut ready);
        let kind = if let Err(err) = result {
            let message = format!("{err:#}");
            worker_capture.active.store(false, Ordering::SeqCst);
            *worker_capture.failure.lock().unwrap() = Some(message.clone());
            if ready {
                Some(RecordingEventKind::Error(message))
            } else {
                let _ = ready_sender.send(Err(message));
                None
            }
        } else if worker_capture.limit_reached.load(Ordering::SeqCst) {
            Some(RecordingEventKind::LimitReached)
        } else {
            None
        };
        if let Some(kind) = kind {
            emit_recording_event(&event_callback, RecordingEvent { session_id, kind });
        }
    });
    let next = Recording {
        capture,
        started_at: Instant::now(),
        worker,
    };
    match ready_receiver.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(next),
        Ok(Err(message)) => {
            next.capture.active.store(false, Ordering::SeqCst);
            let _ = next.worker.join();
            anyhow::bail!(message)
        }
        Err(_) => {
            next.capture.active.store(false, Ordering::SeqCst);
            // The detached worker retains its private cancellation and guard.
            anyhow::bail!("Microphone did not start in time. Check your device and Windows microphone permissions.")
        }
    }
}

fn write_wav(samples: &[i16], sample_rate: u32) -> anyhow::Result<tempfile::TempPath> {
    write_wav_in(samples, sample_rate, &temporary_audio_dir()?)
}

fn write_wav_in(
    samples: &[i16],
    sample_rate: u32,
    directory: &Path,
) -> anyhow::Result<tempfile::TempPath> {
    let path = tempfile::Builder::new()
        .prefix("vellora-")
        .rand_bytes(12)
        .suffix(".wav")
        .tempfile_in(directory)
        .context("Could not create temporary audio file")?
        .into_temp_path();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(&path, spec).context("Could not create WAV writer")?;
    for &sample in samples {
        writer
            .write_sample(sample)
            .context("Could not write WAV sample")?;
    }
    writer.finalize().context("Could not finalize WAV file")?;
    // TempPath deletes the file on success, failure or cancellation in its caller.
    Ok(path)
}

fn temporary_audio_dir() -> anyhow::Result<PathBuf> {
    let app_directory = std::env::temp_dir().join("Vellora");
    ensure_regular_directory(&app_directory)?;
    let audio_directory = app_directory.join("audio");
    ensure_regular_directory(&audio_directory)?;
    Ok(audio_directory)
}

fn ensure_regular_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !is_reparse_point(&metadata),
                "Temporary audio directory must be a regular directory"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).context("Could not create temporary audio directory")?;
            let metadata = fs::symlink_metadata(path)
                .context("Could not inspect temporary audio directory")?;
            anyhow::ensure!(
                metadata.is_dir() && !is_reparse_point(&metadata),
                "Temporary audio directory must be a regular directory"
            );
        }
        Err(error) => return Err(error).context("Could not inspect temporary audio directory"),
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Remove only app-owned recordings left behind by an earlier crash. Active
/// recordings are bounded to five minutes, so a 24-hour age guard keeps current
/// recordings and concurrently running developer builds untouched.
pub fn cleanup_stale_audio_files() -> anyhow::Result<usize> {
    cleanup_stale_audio_in(&temporary_audio_dir()?, SystemTime::now())
}

fn cleanup_stale_audio_in(directory: &Path, now: SystemTime) -> anyhow::Result<usize> {
    ensure_regular_directory(directory)?;
    let mut count = 0;
    for entry in fs::read_dir(directory).context("Could not read temporary audio directory")? {
        let entry = entry.context("Could not inspect temporary audio directory entry")?;
        let name = entry.file_name();
        let Some(random) = name
            .to_str()
            .and_then(|name| name.strip_prefix("vellora-"))
            .and_then(|name| name.strip_suffix(".wav"))
        else {
            continue;
        };
        if random.len() != 12 || !random.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            continue;
        }
        // Inspect without following links. Never descend into another directory
        // or a Windows junction/reparse point, even if its name resembles ours.
        let metadata =
            fs::symlink_metadata(entry.path()).context("Could not inspect temporary recording")?;
        if !metadata.is_file() || is_reparse_point(&metadata) {
            continue;
        }
        let modified = metadata
            .modified()
            .context("Could not inspect recording age")?;
        if now.duration_since(modified).unwrap_or_default() >= Duration::from_secs(24 * 60 * 60) {
            fs::remove_file(entry.path()).context("Could not remove stale temporary recording")?;
            count += 1;
        }
    }
    Ok(count)
}

fn emit_recording_event(
    callback: &Arc<Mutex<Option<RecordingEventCallback>>>,
    event: RecordingEvent,
) {
    if let Some(callback) = callback.lock().unwrap().clone() {
        callback(event);
    }
}

fn record_loop(
    capture: Arc<Capture>,
    audio_level_callback: Arc<Mutex<Option<AudioLevelCallback>>>,
    ready_sender: &mpsc::SyncSender<Result<(), String>>,
    ready: &mut bool,
) -> anyhow::Result<()> {
    if !capture.active.load(Ordering::SeqCst) {
        return Ok(());
    }
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("No default microphone found")?;
    let config = device
        .default_input_config()
        .context("Could not read default microphone config")?;
    // A timed-out open can return later. Do not continue opening or playing a
    // stream for a capture that its caller has already cancelled.
    if !capture.active.load(Ordering::SeqCst) {
        return Ok(());
    }
    capture
        .sample_rate
        .store(config.sample_rate().0, Ordering::SeqCst);
    let channels = usize::from(config.channels());
    anyhow::ensure!(channels > 0, "Microphone reported no audio channels");
    let sample_limit = recording_sample_limit(config.sample_rate().0);
    let error_capture = capture.clone();
    let err_fn = move |err| {
        *error_capture.failure.lock().unwrap() = Some(format!("Microphone stream failed: {err}"));
        error_capture.active.store(false, Ordering::SeqCst);
    };
    let callback_capture = capture.clone();
    let mut meter = LevelMeter::new(audio_level_callback);
    let stream_config = config.clone().into();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                push_samples(
                    data,
                    channels,
                    &callback_capture,
                    sample_limit,
                    &mut meter,
                    |v| {
                        if v.is_finite() {
                            v.clamp(-1.0, 1.0)
                        } else {
                            0.0
                        }
                    },
                );
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                push_samples(
                    data,
                    channels,
                    &callback_capture,
                    sample_limit,
                    &mut meter,
                    |v| f32::from(v) / 32_768.0,
                );
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                push_samples(
                    data,
                    channels,
                    &callback_capture,
                    sample_limit,
                    &mut meter,
                    |v| (f32::from(v) - 32_768.0) / 32_768.0,
                );
            },
            err_fn,
            None,
        )?,
        other => anyhow::bail!("Unsupported microphone sample format: {other:?}"),
    };
    if !capture.active.load(Ordering::SeqCst) {
        return Ok(());
    }
    stream.play().context("Could not start microphone stream")?;
    *ready = true;
    let _ = ready_sender.send(Ok(()));
    let started_at = Instant::now();
    while capture.active.load(Ordering::SeqCst) {
        if enforce_duration_limit(&capture, started_at.elapsed()) {
            break;
        }
        thread::sleep(Duration::from_millis(30));
    }
    drop(stream);
    if let Some(failure) = capture.failure.lock().unwrap().clone() {
        anyhow::bail!(failure);
    }
    Ok(())
}

fn enforce_duration_limit(capture: &Capture, elapsed: Duration) -> bool {
    if elapsed >= Duration::from_secs(MAX_RECORDING_SECONDS) {
        capture.limit_reached.store(true, Ordering::SeqCst);
        capture.active.store(false, Ordering::SeqCst);
        return true;
    }
    false
}

fn recording_sample_limit(sample_rate: u32) -> usize {
    (u64::from(sample_rate).saturating_mul(MAX_RECORDING_SECONDS)).min(MAX_AUDIO_SAMPLES as u64)
        as usize
}

struct LevelMeter {
    callback: Arc<Mutex<Option<AudioLevelCallback>>>,
    accumulator: f64,
    count: usize,
    last_emit: Instant,
}

impl LevelMeter {
    fn new(callback: Arc<Mutex<Option<AudioLevelCallback>>>) -> Self {
        Self {
            callback,
            accumulator: 0.0,
            count: 0,
            last_emit: Instant::now(),
        }
    }

    fn push(&mut self, sample: f32) {
        self.accumulator += f64::from(sample) * f64::from(sample);
        self.count += 1;
        if self.last_emit.elapsed() < Duration::from_millis(50) || self.count == 0 {
            return;
        }
        let rms = (self.accumulator / self.count as f64).sqrt() as f32;
        let level = (rms / 0.12).clamp(0.0, 1.0);
        self.accumulator = 0.0;
        self.count = 0;
        self.last_emit = Instant::now();
        if let Some(callback) = self.callback.lock().unwrap().clone() {
            callback(level);
        }
    }
}

fn push_samples<T: Copy>(
    data: &[T],
    channels: usize,
    capture: &Capture,
    sample_limit: usize,
    meter: &mut LevelMeter,
    to_f32: impl Fn(T) -> f32,
) {
    if !capture.active.load(Ordering::SeqCst) || channels == 0 {
        return;
    }
    let mut out = capture.samples.lock().unwrap();
    for frame in data.chunks_exact(channels) {
        if out.len() >= sample_limit {
            capture.limit_reached.store(true, Ordering::SeqCst);
            capture.active.store(false, Ordering::SeqCst);
            break;
        }
        let mono = frame.iter().copied().map(&to_f32).sum::<f32>() / channels as f32;
        meter.push(mono);
        out.push((mono.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_worker_exit(worker_running: &AtomicBool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while worker_running.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "synthetic worker did not exit");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn timed_out_worker_blocks_retries_until_it_actually_exits() {
        let running = Arc::new(AtomicBool::new(false));
        let callback = Arc::new(Mutex::new(None));
        let (release, blocked) = mpsc::channel();
        let first = start_capture_worker(
            running.clone(),
            "old-session".to_string(),
            callback.clone(),
            Duration::from_millis(10),
            move |capture, _, _| {
                blocked.recv().unwrap();
                assert!(!capture.active.load(Ordering::SeqCst));
                Ok(())
            },
        );
        assert!(first.is_err());
        assert!(running.load(Ordering::Acquire));

        let retry_started = Arc::new(AtomicBool::new(false));
        let started = retry_started.clone();
        let retry = start_capture_worker(
            running.clone(),
            "new-session".to_string(),
            callback.clone(),
            Duration::from_millis(10),
            move |_, _, _| {
                started.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(retry.err().unwrap().to_string().contains("still closing"));
        assert!(!retry_started.load(Ordering::SeqCst));
        release.send(()).unwrap();
        wait_for_worker_exit(&running);

        let restarted = start_capture_worker(
            running.clone(),
            "new-session".to_string(),
            callback,
            Duration::from_secs(2),
            |_, ready_sender, ready| {
                *ready = true;
                ready_sender.send(Ok(())).unwrap();
                Ok(())
            },
        )
        .unwrap();
        restarted.worker.join().unwrap();
        assert!(!running.load(Ordering::Acquire));
    }

    #[test]
    fn late_worker_error_keeps_original_session_and_cannot_claim_a_new_one() {
        let running = Arc::new(AtomicBool::new(false));
        let (release, blocked) = mpsc::channel();
        let (events, received) = mpsc::channel();
        let callback: RecordingEventCallback = Arc::new(move |event| {
            events.send(event).unwrap();
        });
        let first = start_capture_worker(
            running.clone(),
            "old-session".to_string(),
            Arc::new(Mutex::new(Some(callback))),
            Duration::from_millis(10),
            move |_, _, ready| {
                blocked.recv().unwrap();
                // A delayed driver can become ready and fail after the caller
                // has already timed out and cancelled its private capture.
                *ready = true;
                anyhow::bail!("synthetic delayed driver failure")
            },
        );
        assert!(first.is_err());
        release.send(()).unwrap();
        let event = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(event.kind, RecordingEventKind::Error(_)));
        assert!(event.belongs_to(Some("old-session")));
        assert!(!event.belongs_to(Some("new-session")));
        assert!(!event.belongs_to(None));
        wait_for_worker_exit(&running);
    }

    #[test]
    fn ready_worker_failure_reports_once_and_stop_consumes_failed_recording() {
        const FAILURE: &str = "synthetic runtime driver failure";
        let recorder = AudioRecorder::new();
        let (release, blocked) = mpsc::channel();
        let (events, received) = mpsc::channel();
        recorder.set_recording_event_callback(Some(Arc::new(move |event| {
            events.send(event).unwrap();
        })));
        let recording = start_capture_worker(
            recorder.worker_running.clone(),
            "runtime-session".to_string(),
            recorder.recording_event_callback.clone(),
            Duration::from_secs(2),
            move |_, ready_sender, ready| {
                *ready = true;
                ready_sender.send(Ok(())).unwrap();
                blocked.recv().unwrap();
                anyhow::bail!(FAILURE)
            },
        )
        .unwrap();
        let capture = recording.capture.clone();
        assert!(capture.active.load(Ordering::SeqCst));
        assert!(recorder.worker_running.load(Ordering::Acquire));
        *recorder.recording.lock().unwrap() = Some(recording);

        release.send(()).unwrap();
        let event = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(event.session_id, "runtime-session");
        match &event.kind {
            RecordingEventKind::Error(message) => assert_eq!(message, FAILURE),
            RecordingEventKind::LimitReached => panic!("failure emitted a recording limit"),
        }
        wait_for_worker_exit(&recorder.worker_running);
        assert!(!capture.active.load(Ordering::SeqCst));
        assert_eq!(capture.failure.lock().unwrap().as_deref(), Some(FAILURE));
        assert_eq!(received.try_recv().unwrap_err(), mpsc::TryRecvError::Empty);

        let failure = recorder.stop_to_wav().err().unwrap();
        assert_eq!(failure.to_string(), FAILURE);
        assert!(capture.failure.lock().unwrap().is_none());
        assert!(recorder.recording.lock().unwrap().is_none());
        assert!(recorder.stop_to_wav().unwrap().is_none());
        assert!(!recorder.worker_running.load(Ordering::Acquire));
        assert_eq!(received.try_recv().unwrap_err(), mpsc::TryRecvError::Empty);
    }

    #[test]
    fn capture_limit_does_not_grow_or_accept_more_samples() {
        let capture = Capture::new();
        let mut meter = LevelMeter::new(Arc::new(Mutex::new(None)));
        push_samples(&[0.5; 16], 2, &capture, 3, &mut meter, |v| v);
        assert_eq!(capture.samples.lock().unwrap().len(), 3);
        assert!(capture.limit_reached.load(Ordering::SeqCst));
        assert!(!capture.active.load(Ordering::SeqCst));
        push_samples(&[0.5; 16], 2, &capture, 3, &mut meter, |v| v);
        assert_eq!(capture.samples.lock().unwrap().len(), 3);
    }

    #[test]
    fn normal_and_high_sample_rates_remain_below_upload_limit() {
        assert_eq!(recording_sample_limit(16_000), 4_800_000);
        for rate in [44_100, 48_000, 96_000, 192_000, u32::MAX] {
            assert!(recording_sample_limit(rate) * 2 + 44 <= MAX_AUDIO_FILE_BYTES as usize);
        }
    }

    #[test]
    fn elapsed_limit_stops_capture_even_without_any_audio_callbacks() {
        let capture = Capture::new();
        assert!(!enforce_duration_limit(&capture, Duration::from_secs(299)));
        assert!(enforce_duration_limit(&capture, Duration::from_secs(300)));
        assert!(!capture.active.load(Ordering::SeqCst));
        assert!(capture.limit_reached.load(Ordering::SeqCst));
        assert!(capture.samples.lock().unwrap().is_empty());
    }

    #[test]
    fn stale_cleanup_removes_only_owned_files_and_keeps_recent_files() {
        let dir = tempfile::tempdir().unwrap();
        let owned = dir.path().join("vellora-aaaaaaaaaaaa.wav");
        let unrelated = dir.path().join("personal.wav");
        let lookalike = dir.path().join("vellora-not-generated.wav");
        let subdirectory = dir.path().join("vellora-bbbbbbbbbbbb.wav");
        fs::write(&owned, "private fixture").unwrap();
        fs::write(&unrelated, "unrelated").unwrap();
        fs::write(&lookalike, "unrelated").unwrap();
        fs::create_dir(&subdirectory).unwrap();
        assert_eq!(
            cleanup_stale_audio_in(dir.path(), SystemTime::now()).unwrap(),
            0
        );
        let future = SystemTime::now() + Duration::from_secs(25 * 60 * 60);
        assert_eq!(cleanup_stale_audio_in(dir.path(), future).unwrap(), 1);
        assert!(!owned.exists());
        assert!(unrelated.exists());
        assert!(lookalike.exists());
        assert!(subdirectory.is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn stale_cleanup_does_not_follow_directory_junction() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let protected = outside.path().join("vellora-aaaaaaaaaaaa.wav");
        fs::write(&protected, "protected fixture").unwrap();
        let junction = dir.path().join("vellora-bbbbbbbbbbbb.wav");
        let result = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(result.status.success());
        let future = SystemTime::now() + Duration::from_secs(25 * 60 * 60);
        assert_eq!(cleanup_stale_audio_in(dir.path(), future).unwrap(), 0);
        assert!(cleanup_stale_audio_in(&junction, future).is_err());
        assert!(protected.exists());
        fs::remove_dir(junction).unwrap();
    }

    #[test]
    fn wav_has_correct_samples_and_is_removed_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_wav_in(&[-300, 0, 300], 16_000, directory.path()).unwrap();
        let retained_path = path.to_path_buf();
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(
            reader
                .samples::<i16>()
                .map(Result::unwrap)
                .collect::<Vec<_>>(),
            [-300, 0, 300]
        );
        drop(reader);
        drop(path);
        assert!(!retained_path.exists());
    }
}
