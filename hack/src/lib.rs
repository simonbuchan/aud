use cpal::{Device, OutputCallbackInfo, SampleFormat, StreamConfig};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct Config {
    device: Device,
    config: StreamConfig,
}

impl Config {
    pub fn get() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("missing output device");
        let config_range = device.supported_output_configs().unwrap()
            .find(|config| config.sample_format() == SampleFormat::F32)
            .expect("no f32 format support");
        let config = config_range.with_max_sample_rate().config();
        Self { device, config }
    }

    pub fn channels(&self) -> u32 {
        self.config.channels.into()
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }

    pub fn create_stream(&self, mut f: impl FnMut(&mut [f32], &OutputCallbackInfo) + Send + 'static) -> Stream {
        return imp(&self.device, &self.config, Box::new(move |buf, info| f(buf, info)));

        fn imp(device: &Device, config: &StreamConfig, f: Box<dyn FnMut(&mut [f32], &OutputCallbackInfo) + Send>) -> Stream {
            let stream = device.build_output_stream(
                &config,
                f,
                |error| panic!("{error}"),
            ).unwrap();
            Stream(stream)
        }
    }
}

pub struct Stream(cpal::Stream);

impl Stream {
    pub fn play(&self) {
        self.0.play().unwrap();
    }
}