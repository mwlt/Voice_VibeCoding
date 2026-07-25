//! IMA/DVI ADPCM 解码器 — 纯 Rust 实现
//!
//! 将小米遥控器 ATVV 语音流（IMA/DVI ADPCM 16kHz 4-bit）
//! 解码为标准 PCM (48kHz 16-bit signed)

/// IMA ADPCM 步长索引表
const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31,
    34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130,
    143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411,
    1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026,
    4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487,
    12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794,
    32767,
];

/// IMA ADPCM 索引调整表
const INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// DVI ADPCM 步长表（变体）
const DVI_STEP_TABLE: [i32; 89] = STEP_TABLE;

/// DVI ADPCM 索引调整表（变体）
const DVI_INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// ADPCM 解码器状态
#[derive(Debug, Clone)]
pub struct AdpcmDecoder {
    /// 当前预测值
    predictor: i32,
    /// 当前步长索引 (0-88)
    step_index: i32,
    /// 是否使用 DVI 变体
    use_dvi: bool,
}

impl AdpcmDecoder {
    /// 创建 IMA ADPCM 解码器
    pub fn new_ima() -> Self {
        Self { predictor: 0, step_index: 0, use_dvi: false }
    }

    /// 创建 DVI ADPCM 解码器
    pub fn new_dvi() -> Self {
        Self { predictor: 0, step_index: 0, use_dvi: true }
    }

    /// 解码单个 4-bit 采样值
    fn decode_nibble(&mut self, nibble: u8) -> i16 {
        let step = if self.use_dvi { DVI_STEP_TABLE } else { STEP_TABLE };
        let idx_adj = if self.use_dvi { DVI_INDEX_TABLE } else { INDEX_TABLE };

        // 确保步长索引不越界
        let step_val = step[self.step_index.clamp(0, 88) as usize];

        // 计算差值
        let mut diff = step_val >> 3;
        if nibble & 4 != 0 { diff += step_val; }
        if nibble & 2 != 0 { diff += step_val >> 1; }
        if nibble & 1 != 0 { diff += step_val >> 2; }

        // 根据符号位调整预测值
        if nibble & 8 != 0 {
            self.predictor = (self.predictor - diff).clamp(-32768, 32767);
        } else {
            self.predictor = (self.predictor + diff).clamp(-32768, 32767);
        }

        // 更新步长索引
        self.step_index = (self.step_index + idx_adj[nibble as usize]).clamp(0, 88);

        self.predictor as i16
    }

    /// 解码 ADPCM 字节块 → PCM 16-bit signed
    ///
    /// 每字节包含 2 个 4-bit 采样（高位在前）
    pub fn decode_block(&mut self, adpcm_data: &[u8]) -> Vec<i16> {
        let mut samples = Vec::with_capacity(adpcm_data.len() * 2);
        for &byte in adpcm_data {
            // 高位 nibble 先解码
            let high = (byte >> 4) & 0x0F;
            samples.push(self.decode_nibble(high));
            // 低位 nibble 后解码
            let low = byte & 0x0F;
            samples.push(self.decode_nibble(low));
        }
        samples
    }

    /// 解码 ADPCM 并上采样到 48kHz（线性插值）
    ///
    /// 输入: 16kHz 4-bit ADPCM
    /// 输出: 48kHz 16-bit PCM（3x 上采样）
    pub fn decode_and_upsample(&mut self, adpcm_data: &[u8]) -> Vec<i16> {
        let samples_16k = self.decode_block(adpcm_data);
        let ratio = 3; // 48kHz / 16kHz

        let mut samples_48k = Vec::with_capacity(samples_16k.len() * ratio);

        for i in 0..samples_16k.len() {
            let current = samples_16k[i];
            samples_48k.push(current);

            if i + 1 < samples_16k.len() {
                let next = samples_16k[i + 1];
                // 线性插值中间值
                for j in 1..ratio {
                    let frac = j as f32 / ratio as f32;
                    let interpolated = (current as f32 * (1.0 - frac) + next as f32 * frac) as i16;
                    samples_48k.push(interpolated);
                }
            } else {
                // 最后一个采样：复制填充
                for _ in 1..ratio {
                    samples_48k.push(current);
                }
            }
        }

        samples_48k
    }

    /// 重置解码器状态（对齐 Python `AdpcmDecoder.reset(predictor, step_index)`）
    pub fn reset(&mut self) {
        self.reset_with(0, 0);
    }

    pub fn reset_with(&mut self, predictor: i32, step_index: i32) {
        self.predictor = predictor.clamp(-32768, 32767);
        self.step_index = step_index.clamp(0, 88);
    }

    /// 对齐 Python `decode_bytes`
    pub fn decode_bytes(&mut self, adpcm_data: &[u8]) -> Vec<i16> {
        self.decode_block(adpcm_data)
    }
}

/// 对齐 Python `postprocess`：增益 + 软限幅
pub fn postprocess(samples: &[i16], gain_db: f32) -> Vec<i16> {
    let gain = 10f32.powf(gain_db / 20.0);
    samples
        .iter()
        .map(|&s| {
            let v = (s as f32 * gain).round();
            v.clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

impl Default for AdpcmDecoder {
    fn default() -> Self {
        Self::new_ima()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_silence() {
        let mut decoder = AdpcmDecoder::new_ima();
        // 0x88 is the standard ADPCM silence nibble (0b1000_1000 → no change)
        let silence = vec![0x88u8; 100];
        let samples = decoder.decode_block(&silence);
        // With proper silence data, values should stay near 0
        assert!(samples.iter().all(|&s| s.abs() < 100));
    }

    #[test]
    fn test_decode_block_length() {
        let mut decoder = AdpcmDecoder::new_ima();
        let data = vec![0x00u8; 50];
        let samples = decoder.decode_block(&data);
        assert_eq!(samples.len(), 100); // 每字节 = 2 采样
    }

    #[test]
    fn test_upsample_length() {
        let mut decoder = AdpcmDecoder::new_ima();
        let data = vec![0x00u8; 10];
        let samples = decoder.decode_and_upsample(&data);
        assert_eq!(samples.len(), 60); // 10 * 2 * 3 = 60
    }

    #[test]
    fn test_reset_state() {
        let mut decoder = AdpcmDecoder::new_ima();
        decoder.predictor = 1000;
        decoder.step_index = 50;
        decoder.reset();
        assert_eq!(decoder.predictor, 0);
        assert_eq!(decoder.step_index, 0);
    }

    #[test]
    fn test_dvi_mode() {
        let mut ima = AdpcmDecoder::new_ima();
        let mut dvi = AdpcmDecoder::new_dvi();
        let data = vec![0x12, 0x34, 0x56];

        // DVI 和 IMA 在相同输入下可能产生不同输出（取决于初始状态）
        let ima_samples = ima.decode_block(&data);
        dvi.reset();
        let dvi_samples = dvi.decode_block(&data);
        assert_eq!(ima_samples.len(), dvi_samples.len());
    }
}
