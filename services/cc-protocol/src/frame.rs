//! agent ↔ 中继 TCP 链路的成帧：`u32 LE 长度 + JSON 字节`。
//!
//! 两端都是自家代码，刻意不做 HTTP/WS；上限 [`MAX_FRAME_BYTES`] 之外的字节直接判为对端
//! 异常。解码器按块喂入、按帧弹出，天然支持跨读的分片。

use serde::{Serialize, de::DeserializeOwned};

/// 单帧上限；超过即断链。
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

const LENGTH_PREFIX: usize = 4;

/// 成帧/解帧失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// 帧超过 [`MAX_FRAME_BYTES`]。
    TooLarge(usize),
    /// 解码器已进入错误态（收到非法长度后必须重置连接）。
    Poisoned,
    /// JSON 序列化失败。
    Encode(String),
    /// JSON 反序列化失败。
    Decode(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(size) => write!(formatter, "frame of {size} bytes exceeds limit"),
            Self::Poisoned => formatter.write_str("frame decoder poisoned by an invalid length"),
            Self::Encode(message) => write!(formatter, "could not encode frame: {message}"),
            Self::Decode(message) => write!(formatter, "could not decode frame: {message}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// 把一条消息编码成完整帧（含长度前缀）。
pub fn encode_frame(message: &impl Serialize) -> Result<Vec<u8>, FrameError> {
    let payload =
        serde_json::to_vec(message).map_err(|error| FrameError::Encode(error.to_string()))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(payload.len()));
    }
    let mut frame = Vec::with_capacity(LENGTH_PREFIX + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// 把一帧载荷解码为消息。
pub fn decode_frame<T: DeserializeOwned>(payload: &[u8]) -> Result<T, FrameError> {
    serde_json::from_slice(payload).map_err(|error| FrameError::Decode(error.to_string()))
}

/// 流式帧解码器：喂入任意分片的字节，弹出完整帧。
#[derive(Debug)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
    poisoned: bool,
}

impl FrameDecoder {
    /// 创建空解码器。
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            poisoned: false,
        }
    }

    /// 喂入一段字节；长度非法或缓冲超限时进入错误态。
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        if self.poisoned {
            return Err(FrameError::Poisoned);
        }
        if self.buffer.len() + bytes.len() > LENGTH_PREFIX + MAX_FRAME_BYTES {
            self.poisoned = true;
            let offending = self.buffer.len() + bytes.len();
            self.buffer.clear();
            return Err(FrameError::TooLarge(offending));
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    /// 弹出下一条完整帧的载荷；数据不足时返回 `None`。
    pub fn pop_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        if self.poisoned {
            return Err(FrameError::Poisoned);
        }
        if self.buffer.len() < LENGTH_PREFIX {
            return Ok(None);
        }
        let length = u32::from_le_bytes([
            self.buffer[0],
            self.buffer[1],
            self.buffer[2],
            self.buffer[3],
        ]) as usize;
        if length > MAX_FRAME_BYTES {
            self.poisoned = true;
            self.buffer.clear();
            return Err(FrameError::TooLarge(length));
        }
        if self.buffer.len() < LENGTH_PREFIX + length {
            return Ok(None);
        }
        let payload = self.buffer[LENGTH_PREFIX..LENGTH_PREFIX + length].to_vec();
        self.buffer.drain(..LENGTH_PREFIX + length);
        Ok(Some(payload))
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_roundtrip_across_split_reads() {
        let first = encode_frame(&serde_json::json!({"type": "ping"})).expect("encode");
        let second = encode_frame(&serde_json::json!({"type": "pong"})).expect("encode");
        let mut stream = first.clone();
        stream.extend_from_slice(&second);

        // 逐字节喂入，验证跨读分片。
        let mut decoder = FrameDecoder::new();
        for byte in &stream {
            decoder.push(std::slice::from_ref(byte)).expect("push");
        }
        let mut payloads = Vec::new();
        while let Some(payload) = decoder.pop_frame().expect("pop") {
            payloads.push(payload);
        }
        assert_eq!(payloads.len(), 2);
        assert_eq!(
            decode_frame::<serde_json::Value>(&payloads[0]).expect("decode")["type"],
            "ping"
        );
        assert_eq!(
            decode_frame::<serde_json::Value>(&payloads[1]).expect("decode")["type"],
            "pong"
        );
    }

    #[test]
    fn oversize_declared_length_poisons_the_decoder() {
        let mut decoder = FrameDecoder::new();
        let mut bogus = (MAX_FRAME_BYTES as u32 + 1).to_le_bytes().to_vec();
        bogus.extend_from_slice(b"junk");
        assert!(matches!(
            decoder.pop_frame_after_push(&bogus),
            Err(FrameError::TooLarge(_))
        ));
        assert!(matches!(decoder.push(b"x"), Err(FrameError::Poisoned)));
    }

    #[test]
    fn incomplete_frame_returns_none() {
        let frame = encode_frame(&serde_json::json!({"type": "hello"})).expect("encode");
        let mut decoder = FrameDecoder::new();
        decoder.push(&frame[..frame.len() - 1]).expect("push");
        assert!(decoder.pop_frame().expect("pop").is_none());
        decoder.push(&frame[frame.len() - 1..]).expect("push");
        assert!(decoder.pop_frame().expect("pop").is_some());
    }

    impl FrameDecoder {
        // 测试辅助：喂入后立即尝试弹帧。
        fn pop_frame_after_push(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>, FrameError> {
            match self.push(bytes) {
                Ok(()) => self.pop_frame(),
                Err(error) => Err(error),
            }
        }
    }
}
