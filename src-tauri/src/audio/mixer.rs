//! 音频混音器 — 将物理麦克风实时路由到 VB-CABLE 虚拟输入
//!
//! 使用 cpal WASAPI 后端，按需创建/销毁音频流。
//! 输入：物理麦克风 (int16, 48kHz, 单声道)
//! 输出：VB-CABLE "CABLE Input" playback endpoint

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct AudioMixer {
    running: Arc<AtomicBool>,
    input_stream: Option<cpal::Stream>,
    output_stream: Option<cpal::Stream>,
}

impl AudioMixer {
    pub fn new() -> Self {
        Self { running: Arc::new(AtomicBool::new(false)), input_stream: None, output_stream: None }
    }

    /// 打开混音器：从指定麦克风读取音频，写入 VB-CABLE
    pub fn open(&mut self, mic_name: Option<&str>, cable_name: Option<&str>) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("混音器已在运行".into());
        }

        let host = cpal::default_host();

        // 查找输入设备（麦克风）
        let input_device = if let Some(name) = mic_name {
            find_device(&host, name, true)?
        } else {
            host.default_input_device()
                .ok_or("未找到默认麦克风设备")?
        };

        // 查找输出设备（VB-CABLE CABLE Input）
        let output_device = if let Some(name) = cable_name {
            find_device(&host, name, false)?
        } else {
            find_device(&host, "CABLE Input", false)
                .or_else(|_| host.default_output_device()
                    .ok_or("未找到输出设备"))
                .map_err(|e| format!("未找到 VB-CABLE 设备: {}", e))?
        };

        log::info!("Input device: {}", input_device.name().unwrap_or_default());
        log::info!("Output device: {}", output_device.name().unwrap_or_default());

        // 获取输入设备支持的配置
        let input_config = input_device
            .default_input_config()
            .map_err(|e| format!("获取输入配置失败: {}", e))?;

        let output_config = output_device
            .default_output_config()
            .map_err(|e| format!("获取输出配置失败: {}", e))?;

        log::info!("Input config: {:?}", input_config);
        log::info!("Output config: {:?}", output_config);

        // 音频缓冲区：共享环形缓冲
        let buffer: Arc<std::sync::Mutex<Vec<i16>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let buffer_clone = Arc::clone(&buffer);
        let running = Arc::clone(&self.running);
        self.running.store(true, Ordering::SeqCst);

        // 创建输入流：从麦克风读取
        let running_in = Arc::clone(&running);
        let input_stream = input_device
            .build_input_stream(
                &input_config.config(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if !running_in.load(Ordering::SeqCst) { return; }
                    if let Ok(mut buf) = buffer.lock() {
                        buf.extend_from_slice(data);
                        // 限制缓冲区大小（约 200ms）
                        if buf.len() > input_config.sample_rate().0 as usize / 5 {
                            buf.drain(..data.len());
                        }
                    }
                },
                |err| log::error!("Input stream error: {}", err),
                None,
            )
            .map_err(|e| format!("创建输入流失败: {}", e))?;

        // 创建输出流：写入 VB-CABLE
        let running_out = Arc::clone(&running);
        let output_stream = output_device
            .build_output_stream(
                &output_config.config(),
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    if !running_out.load(Ordering::SeqCst) { return; }
                    if let Ok(mut buf) = buffer_clone.lock() {
                        let available = buf.len().min(data.len());
                        if available > 0 {
                            data[..available].copy_from_slice(&buf[..available]);
                            buf.drain(..available);
                        } else {
                            // 无数据时输出静音
                            data.fill(0);
                        }
                    }
                },
                |err| log::error!("Output stream error: {}", err),
                None,
            )
            .map_err(|e| format!("创建输出流失败: {}", e))?;

        // 启动流
        input_stream.play().map_err(|e| format!("启动输入流失败: {}", e))?;
        output_stream.play().map_err(|e| format!("启动输出流失败: {}", e))?;

        self.input_stream = Some(input_stream);
        self.output_stream = Some(output_stream);

        log::info!("Audio mixer opened: mic -> VB-CABLE");
        Ok(())
    }

    pub fn close(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.input_stream = None;
        self.output_stream = None;
        log::info!("Audio mixer closed");
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 获取可用的输入设备列表
    pub fn list_input_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.input_devices()
            .map(|devices| {
                devices
                    .filter_map(|d| d.name().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 获取可用的输出设备列表
    pub fn list_output_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.output_devices()
            .map(|devices| {
                devices
                    .filter_map(|d| d.name().ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Default for AudioMixer {
    fn default() -> Self { Self::new() }
}

impl Drop for AudioMixer {
    fn drop(&mut self) { self.close(); }
}

/// 通过名称查找音频设备
fn find_device(host: &cpal::Host, name: &str, is_input: bool) -> Result<cpal::Device, String> {
    let devices = if is_input {
        host.input_devices().map_err(|e| format!("枚举输入设备失败: {}", e))?
    } else {
        host.output_devices().map_err(|e| format!("枚举输出设备失败: {}", e))?
    };

    for device in devices {
        if let Ok(dev_name) = device.name() {
            if dev_name.to_lowercase().contains(&name.to_lowercase()) {
                return Ok(device);
            }
        }
    }

    Err(format!("未找到设备: {}", name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_mixer() {
        let mixer = AudioMixer::new();
        assert!(!mixer.is_running());
    }

    #[test]
    fn test_list_devices() {
        let inputs = AudioMixer::list_input_devices();
        let outputs = AudioMixer::list_output_devices();
        // At minimum, there should be some devices on any system
        log::info!("Input devices: {:?}", inputs);
        log::info!("Output devices: {:?}", outputs);
    }
}
