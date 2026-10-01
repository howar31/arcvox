//! cpal-backed `AudioSource`. The stream callback only converts to mono f32
//! and forwards; everything else happens on the controller thread.

use std::sync::mpsc::Sender;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Sample, SampleFormat, StreamConfig};

use super::{AudioError, AudioHandle, AudioMsg, AudioSource};
use crate::settings::MicrophoneChoice;

#[derive(Default)]
pub struct CpalSource;

struct CpalHandle {
    _stream: cpal::Stream,
}

impl AudioHandle for CpalHandle {
    fn stop(self: Box<Self>) {
        // Dropping the stream stops it.
    }
}

fn device_name(d: &Device) -> String {
    d.description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_default()
}

fn pick(host: &cpal::Host, choice: &MicrophoneChoice) -> Option<Device> {
    let devices: Vec<Device> = host.input_devices().ok()?.collect();
    match choice {
        MicrophoneChoice::SystemDefault => host.default_input_device(),
        MicrophoneChoice::BuiltIn => devices
            .iter()
            .find(|d| {
                let n = device_name(d);
                n.contains("MacBook") || n.contains("Built-in") || n.contains("內建")
            })
            .cloned()
            .or_else(|| host.default_input_device()),
        MicrophoneChoice::Named(name) => devices
            .iter()
            .find(|d| &device_name(d) == name)
            .cloned()
            .or_else(|| host.default_input_device()),
    }
}

fn to_mono_f32<T: cpal::Sample + cpal::SizedSample>(data: &[T], channels: usize) -> Vec<f32>
where
    f32: cpal::FromSample<T>,
{
    let channels = channels.max(1);
    data.chunks(channels)
        .map(|frame| frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32)
        .collect()
}

/// A dropout or a route change is reported by a stream that goes on
/// delivering audio; every other error ends it.
fn stream_event(e: &cpal::Error) -> AudioMsg {
    match e.kind() {
        cpal::ErrorKind::Xrun | cpal::ErrorKind::DeviceChanged => AudioMsg::Glitch(e.to_string()),
        _ => AudioMsg::Error(e.to_string()),
    }
}

/// Builds the input stream for one concrete sample type. Shared by every
/// `SampleFormat` arm in `CpalSource::start` so the mono-mixdown and error
/// forwarding logic exists once.
fn build_stream<T>(
    device: &Device,
    config: StreamConfig,
    rate: u32,
    channels: usize,
    tx: Sender<AudioMsg>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::Sample + cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let err_tx = tx.clone();
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let _ = tx.send(AudioMsg::Frames {
                rate,
                mono: to_mono_f32(data, channels),
            });
        },
        move |e: cpal::Error| {
            let _ = err_tx.send(stream_event(&e));
        },
        None,
    )
}

impl AudioSource for CpalSource {
    fn start(
        &self,
        choice: &MicrophoneChoice,
        tx: Sender<AudioMsg>,
    ) -> Result<Box<dyn AudioHandle>, AudioError> {
        let host = cpal::default_host();
        let device = pick(&host, choice).ok_or(AudioError::NoDevice)?;
        let supported = device
            .default_input_config()
            .map_err(|e| AudioError::Other(e.to_string()))?;
        let rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let stream = match format {
            SampleFormat::F32 => build_stream::<f32>(&device, config, rate, channels, tx),
            SampleFormat::I16 => build_stream::<i16>(&device, config, rate, channels, tx),
            SampleFormat::I32 => build_stream::<i32>(&device, config, rate, channels, tx),
            other => return Err(AudioError::UnsupportedFormat(format!("{other:?}"))),
        }
        .map_err(|e| AudioError::Other(e.to_string()))?;
        stream
            .play()
            .map_err(|e| AudioError::Other(e.to_string()))?;
        Ok(Box::new(CpalHandle { _stream: stream }))
    }

    fn list_devices(&self) -> Vec<String> {
        let host = cpal::default_host();
        host.input_devices()
            .map(|it| {
                it.map(|d| device_name(&d))
                    .filter(|n| !n.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_dropout_or_a_route_change_leaves_the_stream_running() {
        for kind in [cpal::ErrorKind::Xrun, cpal::ErrorKind::DeviceChanged] {
            assert!(matches!(stream_event(&kind.into()), AudioMsg::Glitch(_)));
        }
        assert!(matches!(
            stream_event(&cpal::ErrorKind::DeviceNotAvailable.into()),
            AudioMsg::Error(_)
        ));
    }

    #[test]
    fn mono_mixdown_averages_channels() {
        assert_eq!(to_mono_f32(&[0.2f32, 0.4, -0.2, 0.2], 2), vec![0.3, 0.0]);
        assert_eq!(to_mono_f32(&[i16::MAX, i16::MAX], 1).len(), 2);
        assert!((to_mono_f32(&[i16::MAX], 1)[0] - 1.0).abs() < 1e-3);
    }
}
