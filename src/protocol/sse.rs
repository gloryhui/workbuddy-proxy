//! Pull-based SSE framing: consume at most one frame per call, even when the
//! network hands us a huge chunk. The caller retains the unconsumed bytes.
//!
//! 本模块只负责 SSE 的字节分帧与字段整理，不理解 Responses、Anthropic 或
//! Gemini 的事件语义。上游是 `proxy::native::NativeStream`（源码文件
//! `proxy_native.rs`）持有的
//! `reqwest::Response::bytes_stream()`；下游是 `streaming::Decoder::event`。
//! 这些调用发生在代理请求的异步 body future 内，不另建线程，也没有跨请求
//! 共享的 Parser。每个响应拥有自己的可变 `Parser` 和当前接收 chunk，因此
//! 状态不会从一个客户端泄漏到另一个客户端。
//!
//! 必须区分四层数据：
//! - 网络 chunk 是传输库每次交给 future 的任意字节切片，边界没有语义；
//! - Frame 是 SSE 用一个空行结束的一段原始字节，一个 chunk 可含多个 Frame，
//!   一个 Frame 也可跨很多 chunk；
//! - Event 是从 Frame 的 `event:` 与一条或多条 `data:` 行整理出的 String；
//! - Chat delta 是协议 Decoder 解析 Event 后生成的 OpenAI Chat 增量对象。
//! 本文件只完成前两层到 Event 的转换，不能把“收到 chunk”直接称为“收到事件”。
use super::Error;

/// 单个 SSE Frame 最多保留 1 MiB 原始字节。
///
/// 对端在空白行之前超过该上限时立即失败，避免异常上游让单个请求无限占用
/// 内存。上限的单位是字节，不是字符数。
pub const FRAME_LIMIT: usize = 1024 * 1024;

/// 一个响应独占的增量 SSE 分帧器。
///
/// `feed` 可接收任意切分的网络字节；未遇到空行时只把数据留在 `frame` 中。
/// 为限制复制，`feed` 每次最多消费到一个完整 Frame 的结束字节并返回已消费
/// 长度，调用方自行保留同一网络 chunk 中尚未消费的后缀。
#[derive(Default)]
pub struct Parser {
    /// 当前尚未以空行结束的 Frame 原始字节。实际保留内容的 `len()` 不超过
    /// `FRAME_LIMIT`；Vec 的 allocation capacity 不由此注释作精确上限承诺。
    /// 成功产出 Frame 后通过 `mem::take` 交给调用方。
    frame: Vec<u8>,
    /// 当前行是否已有非换行字节。读到换行时用它判断这一行是否为空行，从而
    /// 识别 Frame 边界。
    line_nonempty: bool,
    /// 刚读到 `\r` 后置为 true；若下一字节是 `\n`，它属于同一个 CRLF 换行，
    /// 不能再被算成第二个空行。
    skip_lf: bool,
    /// 是否已经结束流首可选 UTF-8 BOM 的检测。首字节不是 `0xEF` 时会立即
    /// 作为普通正文开始；只有检测到 `0xEF` 才继续要求完整的 `EF BB BF`
    /// 序列。BOM 不会进入 Frame 内容。
    started: bool,
    /// BOM 已匹配的字节数，合法取值范围为 0..=3；只在 `started=false` 时使用。
    bom_bytes: usize,
}

/// 一个已完整分帧、尚待解释的 SSE Frame。
///
/// 内部 `Vec<u8>` 由 Frame 独占。`into_event` 消耗这个值并在原分配上整理
/// `data`，避免同时长期保留“原始 Frame 字符串”和“Event 字符串”两份大副本。
pub struct Frame(Vec<u8>);
/// 从 SSE Frame 中提取的协议事件输入。
///
/// `kind` 来自 `event:`，缺省为 `message`；`data` 是多条 `data:` 行以换行
/// 连接后的内容。字段使用拥有所有权的 `String`，因为来源 Frame 随后即可
/// 释放。
#[derive(Debug, PartialEq, Eq)]
pub struct Event {
    /// SSE 事件名，不是后续 JSON 内的 `type`；协议适配器会分别校验二者。
    pub kind: String,
    /// 原始 event-data 文本。此处尚未执行 JSON 解析，也可能是 `[DONE]`。
    pub data: String,
}

impl Parser {
    /// 从当前网络 chunk 中消费字节，并最多返回一个完整 Frame。
    ///
    /// 返回值的 `usize` 是从 `chunk` 开头实际消费的字节数；`Some(Frame)` 表示
    /// 恰好在这次消费范围中遇到了一个空行。调用方必须像
    /// `proxy::native::NativeStream` 一样维护 offset，并用
    /// `&chunk[consumed..]` 再调用，直到处理完当前 chunk。
    ///
    /// 本函数刻意不把多个 Frame 排进内部队列：慢消费者不会导致第二个 Frame
    /// 在 Parser 中无限堆积，也不会因为一次很大的网络 chunk 而复制整块数据。
    /// `\n`、`\r\n` 和单独 `\r` 都作为换行处理；空行结束 Frame。
    ///
    /// 副作用只有更新本 Parser 的缓冲与换行状态。非法 BOM 序列或超限 Frame
    /// 会返回结构化上游错误。普通正文的 UTF-8 合法性由
    /// `Frame::into_event` 检查；调用方遇到这里的分帧错误应转换为错误事件，
    /// 不能继续发送成功 DONE。
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(usize, Option<Frame>), Error> {
        for (i, &b) in chunk.iter().enumerate() {
            // 流的整体起始位置最多允许一个 UTF-8 BOM。BOM 可能被网络拆成
            // 多个 chunk，所以必须由 Parser 自己记住已匹配到第几个字节。
            if !self.started {
                if self.bom_bytes > 0 || b == 0xef {
                    let expected = [0xef, 0xbb, 0xbf][self.bom_bytes];
                    if b != expected {
                        return Err(Error::upstream("invalid_sse_utf8"));
                    }
                    self.bom_bytes += 1;
                    if self.bom_bytes == 3 {
                        self.started = true;
                        self.bom_bytes = 0;
                    }
                    continue;
                }
                self.started = true;
            }
            // CRLF 的第二半不是新的行分隔，也不能触发“空行结束 Frame”。
            if self.skip_lf {
                self.skip_lf = false;
                if b == b'\n' {
                    continue;
                }
            }
            if self.frame.len() >= FRAME_LIMIT {
                return Err(Error::upstream("sse_frame_too_large"));
            }
            self.frame.push(b);
            if b == b'\r' || b == b'\n' {
                self.skip_lf = b == b'\r';
                if !self.line_nonempty {
                    // 当前换行之前没有任何非换行字节，因此这是分隔 Frame 的
                    // 空行。只消费到该换行，不越过网络 chunk 的后续内容。
                    return Ok((i + 1, Some(Frame(std::mem::take(&mut self.frame)))));
                }
                self.line_nonempty = false;
            } else {
                self.line_nonempty = true;
            }
        }
        Ok((chunk.len(), None))
    }

    /// 校验传输 EOF 时 SSE Parser 是否停在可接受边界。
    ///
    /// Parser 内部仍有未结束 Frame，或流停在半个 BOM 时，代表上游传输被截断，
    /// 返回 `incomplete_sse_frame`。这是传输/分帧完整性检查，不等于协议终态；
    /// 即使 Parser 返回 Ok，调用方仍必须在 clean EOF 后运行
    /// `Decoder::finish`，由后者确认对应协议的 finish 标记和 final snapshot。
    /// Replay `Pending` 与请求工具约束的最终校验发生在 HTTP 集成层
    /// `proxy::native::pending`，不在 Parser。
    pub fn finish(&self) -> Result<(), Error> {
        if !self.frame.is_empty() || self.bom_bytes != 0 {
            return Err(Error::upstream("incomplete_sse_frame"));
        }
        Ok(())
    }

    #[cfg(test)]
    fn resident_bytes(&self) -> usize {
        self.frame.capacity()
    }
}

impl Frame {
    /// Remove framing in place. The data String owns the existing allocation;
    /// a second 1MiB JSON string is not built alongside a retained raw frame.
    ///
    /// 本方法消耗 Frame，因此原 `Vec<u8>` 的所有权在这里转入解析过程。它先
    /// 整体校验 UTF-8，再逐行走访字段：`event:` 最多 128 字节；多条
    /// `data:` 仅去掉可选的一个前导空格，并以 `\n` 拼接；注释、`id`、`retry`
    /// 和未知字段不进入业务 Event。完全没有 `data:` 的 Frame 返回 `None`，
    /// 例如纯注释或心跳行。
    ///
    /// 这里不解析 JSON，也不把 `data: [DONE]` 特判为成功。事件名称、结束
    /// 标记和错误都必须由具体协议的 Decoder 结合状态机判断。
    pub fn into_event(mut self) -> Result<Option<Event>, Error> {
        std::str::from_utf8(&self.0).map_err(|_| Error::upstream("invalid_sse_utf8"))?;
        let mut kind = String::from("message");
        let mut read = 0;
        let mut write = 0;
        let mut data_seen = false;
        while read < self.0.len() {
            let start = read;
            while read < self.0.len() && !matches!(self.0[read], b'\r' | b'\n') {
                read += 1;
            }
            let end = read;
            while read < self.0.len() && matches!(self.0[read], b'\r' | b'\n') {
                read += 1;
            }
            if start == end || self.0[start] == b':' {
                continue;
            }
            let colon = self.0[start..end]
                .iter()
                .position(|b| *b == b':')
                .map(|v| start + v);
            let (key_end, mut value) = colon.map_or((end, end), |v| (v, v + 1));
            if value < end && self.0[value] == b' ' {
                value += 1;
            }
            match &self.0[start..key_end] {
                b"event" => {
                    if end - value > 128 {
                        return Err(Error::upstream("invalid_sse_event"));
                    }
                    kind = std::str::from_utf8(&self.0[value..end])
                        .expect("validated UTF-8")
                        .into();
                    if kind.is_empty() {
                        kind = "message".into();
                    }
                }
                b"data" => {
                    data_seen = true;
                    let length = end - value;
                    // 在原 Vec 从 write 位置向前压缩 data 值，避免额外构造一条
                    // 与 Frame 同时存在的大 String。
                    self.0.copy_within(value..end, write);
                    write += length;
                    // Every complete data line had a newline in the raw frame,
                    // leaving space for its SSE concatenation separator.
                    self.0[write] = b'\n';
                    write += 1;
                }
                _ => (), // SSE id/retry/extensions do not carry response data.
            }
        }
        if !data_seen {
            // SSE 注释、仅 id/retry 等字段可能合法，但不会形成协议 Event。
            return Ok(None);
        }
        self.0.truncate(write.saturating_sub(1));
        let data = String::from_utf8(self.0).map_err(|_| Error::upstream("invalid_sse_utf8"))?;
        Ok(Some(Event { kind, data }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collect(chunks: &[&[u8]]) -> Result<Vec<Event>, Error> {
        let mut parser = Parser::default();
        let mut events = vec![];
        for chunk in chunks {
            let mut rest = *chunk;
            while !rest.is_empty() {
                let (used, frame) = parser.feed(rest)?;
                rest = &rest[used..];
                if let Some(frame) = frame
                    && let Some(event) = frame.into_event()?
                {
                    events.push(event);
                }
            }
        }
        parser.finish()?;
        Ok(events)
    }
    #[test]
    fn all_chunk_boundaries_preserve_utf8_bom_crlf_and_multiline_data() {
        let bytes = "\u{feff}: ping\r\nevent: content_block_delta\r\ndata: {\"text\":\"中文\",\r\ndata: \"x\":1}\r\n\r\ndata: next\n\n".as_bytes();
        let expected = vec![
            Event {
                kind: "content_block_delta".into(),
                data: "{\"text\":\"中文\",\n\"x\":1}".into(),
            },
            Event {
                kind: "message".into(),
                data: "next".into(),
            },
        ];
        for i in 0..=bytes.len() {
            assert_eq!(
                collect(&[&bytes[..i], &bytes[i..]]).unwrap(),
                expected,
                "split {i}"
            );
        }
        let chunks = bytes.chunks(1).collect::<Vec<_>>();
        assert_eq!(collect(&chunks).unwrap(), expected);
    }
    #[test]
    fn huge_transport_chunk_yields_only_one_frame_and_partial_eof_is_error() {
        let bytes = b"data: one\n\ndata: two\n\n";
        let mut parser = Parser::default();
        let (consumed, frame) = parser.feed(bytes).unwrap();
        assert_eq!(consumed, 11);
        assert_eq!(frame.unwrap().into_event().unwrap().unwrap().data, "one");
        assert_eq!(parser.resident_bytes(), 0);
        assert_eq!(
            collect(&[b"data: unfinished"]).unwrap_err().code,
            "incomplete_sse_frame"
        );
        assert_eq!(
            collect(&[b"data: \xff\n\n"]).unwrap_err().code,
            "invalid_sse_utf8"
        );
        assert!(
            collect(&[b": ping\n\ndata:\n\n"]).unwrap()[0]
                .data
                .is_empty()
        );
        assert_eq!(collect(&[b"data: one\r\rdata: two\r\r"]).unwrap().len(), 2);
    }
    #[test]
    fn oversized_frames_fail_before_retaining_unbounded_chunks() {
        let mut parser = Parser::default();
        let bytes = vec![b'x'; FRAME_LIMIT + 65536];
        assert_eq!(
            parser.feed(&bytes).err().unwrap().code,
            "sse_frame_too_large"
        );
        assert!(parser.resident_bytes() <= FRAME_LIMIT);
    }
}
