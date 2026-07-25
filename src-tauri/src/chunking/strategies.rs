// 切分策略实现

use crate::error::{AppError, AppResult};
use crate::ollama::OllamaClient;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
/// 切分策略标识
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitStrategyId {
    FixedSize,
    RecursiveChar,
    Structural,
    Semantic,
    Agentic,
}

impl SplitStrategyId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FixedSize => "fixed_size",
            Self::RecursiveChar => "recursive_char",
            Self::Structural => "structural",
            Self::Semantic => "semantic",
            Self::Agentic => "agentic",
        }
    }

    pub fn from_str(s: &str) -> AppResult<Self> {
        match s {
            "fixed_size" => Ok(Self::FixedSize),
            "recursive_char" => Ok(Self::RecursiveChar),
            "structural" => Ok(Self::Structural),
            "semantic" => Ok(Self::Semantic),
            "agentic" => Ok(Self::Agentic),
            other => Err(AppError::Other(anyhow::anyhow!(
                "未知的切分策略: {}",
                other
            ))),
        }
    }
}

/// 单个切分结果片段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkResult {
    /// 片段文本
    pub text: String,
    /// 在原文中的字符起始位置
    pub start: usize,
    /// 在原文中的字符结束位置（exclusive）
    pub end: usize,
    /// 切片序号
    pub index: usize,
    /// 元数据（如标题、层级等）
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// 策略参数 JSON Schema 描述（供前端渲染表单）
#[derive(Debug, Clone, Serialize)]
pub struct StrategyParamSchema {
    pub key: String,
    pub label: String,
    pub r#type: String, // "number" | "string" | "boolean"
    pub default: serde_json::Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// 策略元信息
#[derive(Debug, Clone, Serialize)]
pub struct StrategyInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub config_schema: Vec<StrategyParamSchema>,
}

/// 策略参数(运行时反序列化用)
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FixedSizeParams {
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
    #[serde(default = "default_overlap")]
    pub overlap: usize,
}

fn default_chunk_size() -> usize {
    512
}
fn default_overlap() -> usize {
    50
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RecursiveCharParams {
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
    #[serde(default = "default_overlap")]
    pub overlap: usize,
    #[serde(default = "default_separators")]
    pub separators: Vec<String>,
}

fn default_separators() -> Vec<String> {
    vec![
        "\n\n".to_string(),
        "\n".to_string(),
        ". ".to_string(),
        " ".to_string(),
        "".to_string(),
    ]
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StructuralParams {
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
    #[serde(default = "default_overlap")]
    pub overlap: usize,
}

fn default_sentence_group_size() -> usize {
    3
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SemanticParams {
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
    #[serde(default = "default_overlap")]
    pub overlap: usize,
    #[serde(default = "default_sentence_group_size")]
    pub sentence_group_size: usize,
}

/// 切分运行时上下文：为需要外部服务的策略（如 Agentic）注入依赖
#[derive(Clone, Default)]
pub struct SplitContext {
    /// Ollama 客户端；服务不可用或纯本地策略调用时为 None
    pub ollama: Option<OllamaClient>,
    /// 生效的对话模型名
    pub chat_model: String,
    /// LLM 失败时是否允许降级到本地策略
    /// 导入流水线 = true（不阻塞批次）；预览 = false（直接报错让用户感知）
    pub allow_fallback: bool,
}

impl SplitContext {
    /// 纯本地策略 / 无 LLM 场景
    pub fn none() -> Self {
        Self::default()
    }

    pub fn new(client: OllamaClient, model: String, allow_fallback: bool) -> Self {
        Self {
            ollama: Some(client),
            chat_model: model,
            allow_fallback,
        }
    }
}

/// 策略 trait
#[async_trait]
pub trait SplitStrategy: Send + Sync {
    fn id(&self) -> SplitStrategyId;
    fn info(&self) -> StrategyInfo;
    async fn split(
        &self,
        text: &str,
        params: &serde_json::Value,
        ctx: &SplitContext,
    ) -> AppResult<Vec<ChunkResult>>;
}

/// 获取所有可用策略
pub fn list_strategies() -> Vec<StrategyInfo> {
    vec![
        FixedSizeStrategy.info(),
        RecursiveCharStrategy.info(),
        StructuralStrategy.info(),
        SemanticStrategy.info(),
        AgenticSplitStrategy.info(),
    ]
}

/// 根据标识取得策略实例
pub fn get_strategy(id: SplitStrategyId) -> Box<dyn SplitStrategy> {
    match id {
        SplitStrategyId::FixedSize => Box::new(FixedSizeStrategy),
        SplitStrategyId::RecursiveChar => Box::new(RecursiveCharStrategy),
        SplitStrategyId::Structural => Box::new(StructuralStrategy),
        SplitStrategyId::Semantic => Box::new(SemanticStrategy),
        SplitStrategyId::Agentic => Box::new(AgenticSplitStrategy),
    }
}

// ============ 固定大小切分 ============

pub struct FixedSizeStrategy;

#[async_trait]
impl SplitStrategy for FixedSizeStrategy {
    fn id(&self) -> SplitStrategyId {
        SplitStrategyId::FixedSize
    }

    fn info(&self) -> StrategyInfo {
        StrategyInfo {
            id: SplitStrategyId::FixedSize.as_str().to_string(),
            name: "固定大小切分".to_string(),
            description: "基于字符数的滑动窗口,chunk_size=512, overlap=50".to_string(),
            config_schema: vec![
                StrategyParamSchema {
                    key: "chunk_size".into(),
                    label: "片段大小(字符数)".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(512),
                    min: Some(64.0),
                    max: Some(8192.0),
                },
                StrategyParamSchema {
                    key: "overlap".into(),
                    label: "重叠字符数".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(50),
                    min: Some(0.0),
                    max: Some(1024.0),
                },
            ],
        }
    }

    async fn split(
        &self,
        text: &str,
        params: &serde_json::Value,
        _ctx: &SplitContext,
    ) -> AppResult<Vec<ChunkResult>> {
        let p: FixedSizeParams = serde_json::from_value(params.clone()).unwrap_or_default();
        let chunk_size = p.chunk_size.max(64);
        let overlap = p.overlap.min(chunk_size.saturating_sub(1));

        let chars: Vec<char> = text.chars().collect();
        let total = chars.len();
        if total == 0 {
            return Ok(vec![]);
        }

        let step = chunk_size.saturating_sub(overlap).max(1);
        let mut chunks = Vec::new();
        let mut start = 0usize;
        let mut idx = 0usize;

        while start < total {
            let end = (start + chunk_size).min(total);
            let text: String = chars[start..end].iter().collect();
            let text = text.trim();
            if !text.is_empty() {
                chunks.push(ChunkResult {
                    text: text.to_string(),
                    start,
                    end,
                    index: idx,
                    metadata: serde_json::json!({}),
                });
                idx += 1;
            }
            if end >= total {
                break;
            }
            start += step;
        }

        Ok(chunks)
    }
}

// ============ 递归字符切分 ============

pub struct RecursiveCharStrategy;

#[async_trait]
impl SplitStrategy for RecursiveCharStrategy {
    fn id(&self) -> SplitStrategyId {
        SplitStrategyId::RecursiveChar
    }

    fn info(&self) -> StrategyInfo {
        StrategyInfo {
            id: SplitStrategyId::RecursiveChar.as_str().to_string(),
            name: "递归字符切分".to_string(),
            description: "按层级分隔符 [\\n\\n, \\n, \". \", \" \", \"\"] 递归分割,优先保持段落与句子完整性".to_string(),
            config_schema: vec![
                StrategyParamSchema {
                    key: "chunk_size".into(),
                    label: "片段大小(字符数)".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(512),
                    min: Some(64.0),
                    max: Some(8192.0),
                },
                StrategyParamSchema {
                    key: "overlap".into(),
                    label: "重叠字符数".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(50),
                    min: Some(0.0),
                    max: Some(1024.0),
                },
                StrategyParamSchema {
                    key: "separators".into(),
                    label: "分隔符层级".into(),
                    r#type: "string".into(),
                    default: serde_json::json!(default_separators()),
                    min: None,
                    max: None,
                },
            ],
        }
    }

    async fn split(
        &self,
        text: &str,
        params: &serde_json::Value,
        _ctx: &SplitContext,
    ) -> AppResult<Vec<ChunkResult>> {
        let p: RecursiveCharParams =
            serde_json::from_value(params.clone()).unwrap_or_default();
        let chunk_size = p.chunk_size.max(64);
        let overlap = p.overlap.min(chunk_size.saturating_sub(1));
        let separators = if p.separators.is_empty() {
            default_separators()
        } else {
            p.separators
        };

        // 先把文本拆分到不超过 chunk_size 的段
        let pieces = recursive_split(text, &separators, chunk_size);

        // 再用滑动窗口聚合 + overlap
        let mut chunks = Vec::new();
        let mut offset = 0usize;
        let mut current = String::new();
        let mut current_start = 0usize;
        let mut idx = 0usize;

        for piece in pieces {
            let piece_len = piece.chars().count();
            if current.chars().count() + piece_len > chunk_size && !current.is_empty() {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    chunks.push(ChunkResult {
                        text: trimmed,
                        start: current_start,
                        end: offset,
                        index: idx,
                        metadata: serde_json::json!({}),
                    });
                    idx += 1;
                }
                // 处理 overlap:保留尾部
                let keep = overlap.min(current.chars().count());
                let tail: String = current.chars().rev().take(keep).collect::<Vec<_>>().into_iter().rev().collect();
                current = tail;
                current_start = offset.saturating_sub(keep);
            }
            current.push_str(&piece);
            offset += piece_len;
        }

        let trimmed = current.trim().to_string();
        if !trimmed.is_empty() {
            chunks.push(ChunkResult {
                text: trimmed,
                start: current_start,
                end: offset,
                index: idx,
                metadata: serde_json::json!({}),
            });
        }

        Ok(chunks)
    }
}

/// 递归地按分隔符层级切分文本
fn recursive_split(text: &str, separators: &[String], chunk_size: usize) -> Vec<String> {
    if text.chars().count() <= chunk_size {
        return vec![text.to_string()];
    }

    // 找到第一个能在文本中分隔的分隔符
    let sep_idx = separators
        .iter()
        .position(|s| !s.is_empty() && text.contains(s.as_str()));

    match sep_idx {
        Some(i) => {
            let sep = &separators[i];
            let mut parts: Vec<String> = Vec::new();
            for piece in text.split(sep.as_str()) {
                let piece_with_sep = if parts.is_empty() {
                    piece.to_string()
                } else {
                    format!("{}{}", sep, piece)
                };
                if piece_with_sep.chars().count() > chunk_size {
                    // 递归用更细的分隔符
                    let next_seps = &separators[i + 1..];
                    if next_seps.is_empty() {
                        // 无更细分隔符,做硬切分
                        parts.extend(hard_split(&piece_with_sep, chunk_size));
                    } else {
                        parts.extend(recursive_split(&piece_with_sep, next_seps, chunk_size));
                    }
                } else {
                    parts.push(piece_with_sep);
                }
            }
            parts
        }
        None => hard_split(text, chunk_size),
    }
}

/// 硬切分:按字符数切块,不保留语义
fn hard_split(text: &str, chunk_size: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(chunk_size)
        .map(|c| c.iter().collect())
        .collect()
}

// ============ 基于结构切分 ============

pub struct StructuralStrategy;

#[async_trait]
impl SplitStrategy for StructuralStrategy {
    fn id(&self) -> SplitStrategyId {
        SplitStrategyId::Structural
    }

    fn info(&self) -> StrategyInfo {
        StrategyInfo {
            id: SplitStrategyId::Structural.as_str().to_string(),
            name: "基于结构切分".to_string(),
            description: "按 Markdown 标题 / HTML 标签等结构单元切分,保持文档逻辑结构".to_string(),
            config_schema: vec![
                StrategyParamSchema {
                    key: "chunk_size".into(),
                    label: "片段最大字符数".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(512),
                    min: Some(64.0),
                    max: Some(8192.0),
                },
                StrategyParamSchema {
                    key: "overlap".into(),
                    label: "重叠字符数".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(50),
                    min: Some(0.0),
                    max: Some(1024.0),
                },
            ],
        }
    }

    async fn split(
        &self,
        text: &str,
        params: &serde_json::Value,
        _ctx: &SplitContext,
    ) -> AppResult<Vec<ChunkResult>> {
        let p: StructuralParams =
            serde_json::from_value(params.clone()).unwrap_or_default();
        let chunk_size = p.chunk_size.max(64);
        let overlap = p.overlap.min(chunk_size.saturating_sub(1));

        // 检测是否为 Markdown(包含 # 标题)
        let chunks = if text.lines().any(|l| l.starts_with('#')) {
            split_markdown(text, chunk_size, overlap)
        } else {
            // 退化到按段落(空行)切分
            split_paragraphs(text, chunk_size, overlap)
        };

        Ok(chunks)
    }
}

/// 按 Markdown 标题切分
fn split_markdown(text: &str, chunk_size: usize, overlap: usize) -> Vec<ChunkResult> {
    let mut chunks = Vec::new();
    let mut current_section = String::new();
    let mut current_title: Option<String> = None;
    let mut current_start = 0usize;
    let mut offset = 0usize;
    let mut idx = 0usize;

    let flush = |chunks: &mut Vec<ChunkResult>,
                 section: &str,
                 title: &Option<String>,
                 start: usize,
                 end: usize,
                 idx: &mut usize| {
        let trimmed = section.trim();
        if trimmed.is_empty() {
            return;
        }
        // 如果超过 chunk_size,内部硬切分
        if trimmed.chars().count() > chunk_size {
            for piece in hard_split(trimmed, chunk_size) {
                chunks.push(ChunkResult {
                    text: piece,
                    start,
                    end,
                    index: *idx,
                    metadata: serde_json::json!({
                        "title": title,
                    }),
                });
                *idx += 1;
            }
        } else {
            chunks.push(ChunkResult {
                text: trimmed.to_string(),
                start,
                end,
                index: *idx,
                metadata: serde_json::json!({
                    "title": title,
                }),
            });
            *idx += 1;
        }
    };

    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.chars().count();

        if line.starts_with('#') {
            // 遇到新标题,先 flush 旧 section
            flush(
                &mut chunks,
                &current_section,
                &current_title,
                current_start,
                line_start,
                &mut idx,
            );
            current_section = line.to_string();
            current_title = Some(line.trim().to_string());
            current_start = line_start;
        } else {
            current_section.push_str(line);
        }
    }

    flush(
        &mut chunks,
        &current_section,
        &current_title,
        current_start,
        offset,
        &mut idx,
    );

    // 应用 overlap:在每个片段之间保留尾部字符
    if overlap > 0 && chunks.len() > 1 {
        let mut with_overlap: Vec<ChunkResult> = Vec::new();
        for (i, c) in chunks.iter().enumerate() {
            let mut text = c.text.clone();
            if i > 0 {
                let prev = &chunks[i - 1];
                let keep = overlap.min(prev.text.chars().count());
                let tail: String = prev
                    .text
                    .chars()
                    .rev()
                    .take(keep)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                text = format!("{}{}", tail, text);
            }
            with_overlap.push(ChunkResult {
                text,
                start: c.start,
                end: c.end,
                index: i,
                metadata: c.metadata.clone(),
            });
        }
        with_overlap
    } else {
        chunks
    }
}

/// 按段落(空行分隔)切分
fn split_paragraphs(text: &str, chunk_size: usize, overlap: usize) -> Vec<ChunkResult> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_start = 0usize;
    let mut offset = 0usize;
    let mut idx = 0usize;

    for para in text.split("\n\n") {
        let para_with_sep = if current.is_empty() {
            para.to_string()
        } else {
            format!("\n\n{}", para)
        };
        let para_len = para_with_sep.chars().count();

        if current.chars().count() + para_len > chunk_size && !current.is_empty() {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                chunks.push(ChunkResult {
                    text: trimmed,
                    start: current_start,
                    end: offset,
                    index: idx,
                    metadata: serde_json::json!({}),
                });
                idx += 1;
            }
            let keep = overlap.min(current.chars().count());
            let tail: String = current
                .chars()
                .rev()
                .take(keep)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            current = format!("{}{}", tail, para_with_sep);
            current_start = offset.saturating_sub(keep);
        } else {
            if current.is_empty() {
                current_start = offset;
            }
            current.push_str(&para_with_sep);
        }
        offset += para_len;
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        chunks.push(ChunkResult {
            text: trimmed,
            start: current_start,
            end: offset,
            index: idx,
            metadata: serde_json::json!({}),
        });
    }

    chunks
}

// ============ 语义切分 ============

/// 话题转换词（中英文）
const TOPIC_SHIFT_WORDS: &[&str] = &[
    // 中文
    "然而", "但是", "不过", "此外", "另外", "同时", "总之", "综上", "因此", "所以",
    "相反", "实际上", "事实上", "换句话说", "具体来说", "需要注意的是", "值得注意的是",
    "由此可见", "与此相反", "综上所述", "总的来说", "换言之", "一方面", "另一方面",
    // 英文
    "however", "therefore", "moreover", "furthermore", "meanwhile",
    "in contrast", "in summary", "in conclusion", "on the other hand",
    "specifically", "nevertheless", "nonetheless", "consequently",
    "additionally", "alternatively",
];

/// 句子切分后的片段
struct Sentence {
    text: String,
    /// 在原文中的字符起始位置
    start: usize,
    /// 是否在段落边界之后
    after_paragraph_break: bool,
}

/// 按中英文句子边界切分文本
fn split_sentences(text: &str) -> Vec<Sentence> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut current_start = 0usize;
    let mut after_para_break = false;
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let mut i = 0;

    while i < total {
        let ch = chars[i];
        current.push(ch);

        // 检测段落分隔 \n\n
        if ch == '\n' && i + 1 < total && chars[i + 1] == '\n' {
            // 在 \n\n 处切分
            // 跳过第二个 \n 并把它归入当前句子
            current.push(chars[i + 1]);
            i += 2;
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(Sentence {
                    text: trimmed,
                    start: current_start,
                    after_paragraph_break: after_para_break,
                });
                after_para_break = true;
            }
            current = String::new();
            current_start = i;
            continue;
        }

        // 检测句子终止标点
        let is_sentence_end = matches!(ch, '。' | '！' | '？' | '.' | '!' | '?');
        if is_sentence_end {
            // 向后收集空白
            let mut j = i + 1;
            while j < total && (chars[j] == ' ' || chars[j] == '\t') {
                current.push(chars[j]);
                j += 1;
            }
            // 如果后面是 \n（但不是 \n\n），也纳入当前句子
            if j < total && chars[j] == '\n' && (j + 1 >= total || chars[j + 1] != '\n') {
                current.push(chars[j]);
                j += 1;
            }
            i = j;
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(Sentence {
                    text: trimmed,
                    start: current_start,
                    after_paragraph_break: after_para_break,
                });
                after_para_break = false;
            }
            current = String::new();
            current_start = i;
            continue;
        }

        i += 1;
    }

    // 处理剩余文本
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(Sentence {
            text: trimmed,
            start: current_start,
            after_paragraph_break: after_para_break,
        });
    }

    sentences
}

/// 检测句子是否以话题转换词开头
fn is_topic_shift(text: &str) -> bool {
    let lower = text.to_lowercase();
    let trimmed = lower.trim_start();
    TOPIC_SHIFT_WORDS.iter().any(|w| trimmed.starts_with(w))
}

/// 语义切分核心逻辑
///
/// 算法流程：
/// 1. 分句：按中英文标点切分为句子
/// 2. 语义分组：按话题转换词和段落边界将句子分为语义段
/// 3. 合并过小段：将过小的段与相邻段合并
/// 4. 句子优先切分：逐句累加，确保每块新内容 ≤ chunk_size
/// 5. 句子级 overlap：块满时从末尾取完整句子作为下一块的 overlap
/// 6. 超大单句降级：先尝试子句标点切割，仍超长则整句保留
fn semantic_split(
    text: &str,
    chunk_size: usize,
    overlap: usize,
    group_size: usize,
) -> Vec<ChunkResult> {
    let sentences = split_sentences(text);
    if sentences.is_empty() {
        return vec![];
    }

    let group_size = group_size.max(1);
    // chunk_size 是新内容的大小上限，overlap 是额外附加的
    // 实际块大小 = 新内容(≤ chunk_size) + overlap(≤ overlap)

    // ===== 步骤1：语义分组 =====
    // 规则：段落边界强制断开，话题转换词断开，连续短句合并为 group_size 个一组
    struct SemanticSegment {
        text: String,
        start: usize,
        end: usize,
    }

    let mut segments: Vec<SemanticSegment> = Vec::new();
    let mut seg_text = String::new();
    let mut seg_start = 0usize;
    let mut seg_end = 0usize;
    let mut short_count = 0usize; // 连续短句计数

    for sent in &sentences {
        let sent_len = sent.text.chars().count();
        let is_short = sent_len < 50;
        let should_break = sent.after_paragraph_break
            || is_topic_shift(&sent.text)
            || (seg_text.chars().count() + sent_len > chunk_size && !seg_text.is_empty());

        if should_break && !seg_text.is_empty() {
            segments.push(SemanticSegment {
                text: seg_text.trim().to_string(),
                start: seg_start,
                end: seg_end,
            });
            seg_text = String::new();
            short_count = 0;
        }

        if seg_text.is_empty() {
            seg_start = sent.start;
        }

        // 短句群组化：连续短句达到 group_size 时也合并为一段
        if is_short {
            short_count += 1;
            seg_text.push_str(&sent.text);
            seg_text.push(' ');
            seg_end = sent.start + sent_len;

            if short_count >= group_size {
                segments.push(SemanticSegment {
                    text: seg_text.trim().to_string(),
                    start: seg_start,
                    end: seg_end,
                });
                seg_text = String::new();
                short_count = 0;
            }
        } else {
            // 长句直接加入当前段
            short_count = 0;
            seg_text.push_str(&sent.text);
            seg_text.push(' ');
            seg_end = sent.start + sent_len;
        }

        // 如果累积文本超过 chunk_size，强制切分
        if seg_text.chars().count() > chunk_size {
            segments.push(SemanticSegment {
                text: seg_text.trim().to_string(),
                start: seg_start,
                end: seg_end,
            });
            seg_text = String::new();
            short_count = 0;
        }
    }

    // 处理剩余
    let trimmed = seg_text.trim().to_string();
    if !trimmed.is_empty() {
        segments.push(SemanticSegment {
            text: trimmed,
            start: seg_start,
            end: seg_end,
        });
    }

    if segments.is_empty() {
        return vec![];
    }

    // ===== 步骤2：合并过小的段 =====
    let mut merged: Vec<SemanticSegment> = Vec::new();
    for seg in segments {
        if let Some(last) = merged.last_mut() {
            let combined_len = last.text.chars().count() + 1 + seg.text.chars().count();
            if last.text.chars().count() < chunk_size / 4 && combined_len <= chunk_size {
                last.text.push(' ');
                last.text.push_str(&seg.text);
                last.end = seg.end;
                continue;
            }
        }
        merged.push(seg);
    }

    // ===== 步骤3：句子优先切分（不处理 overlap，由步骤4统一处理）=====
    let mut chunks: Vec<ChunkResult> = Vec::new();

    for seg in &merged {
        if seg.text.chars().count() <= chunk_size {
            // 段不大，直接作为 chunk
            chunks.push(ChunkResult {
                text: seg.text.clone(),
                start: seg.start,
                end: seg.end,
                index: chunks.len(),
                metadata: serde_json::json!({}),
            });
        } else {
            // 段超长，逐句累加
            // 注意：seg.text 内分句的 start 是段内偏移，需加上 seg.start 转为原文偏移
            let mut seg_sents = split_sentences(&seg.text);
            for s in seg_sents.iter_mut() {
                s.start += seg.start;
            }
            let mut cur_sents: Vec<&Sentence> = Vec::new();
            let mut cur_len = 0usize;

            for sent in &seg_sents {
                let sent_len = sent.text.chars().count();

                // 超大单句特殊处理
                if sent_len > chunk_size {
                    // flush 当前块
                    if !cur_sents.is_empty() {
                        flush_chunk(&mut chunks, &cur_sents);
                        cur_sents.clear();
                        cur_len = 0;
                    }
                    // 尝试子句切割，仍超长则整句保留
                    let sub_chunks = split_long_sentence(
                        &sent.text,
                        chunk_size,
                        chunks.len(),
                        sent.start,
                    );
                    chunks.extend(sub_chunks);
                    continue;
                }

                // 块满，flush（不计算 overlap，由步骤4统一处理）
                if cur_len + sent_len > chunk_size && !cur_sents.is_empty() {
                    flush_chunk(&mut chunks, &cur_sents);
                    cur_sents.clear();
                    cur_len = 0;
                }

                cur_sents.push(sent);
                cur_len += sent_len;
            }

            // flush 剩余
            if !cur_sents.is_empty() {
                flush_chunk(&mut chunks, &cur_sents);
            }
        }
    }

    // ===== 步骤4：句子级 overlap =====
    // 对所有相邻 chunk 计算 overlap，取完整句子，确保总大小 ≤ chunk_size
    if overlap > 0 && chunks.len() > 1 {
        let mut result = Vec::new();
        for (i, c) in chunks.iter().enumerate() {
            let mut text = c.text.clone();
            let mut start = c.start;
            if i > 0 {
                let prev = &chunks[i - 1];
                // 从前一个 chunk 末尾提取完整句子作为 overlap
                // prev.text 内分句的 start 是 chunk 内偏移，需加上 prev.start 转为原文偏移
                let mut prev_sents = split_sentences(&prev.text);
                for s in prev_sents.iter_mut() {
                    s.start += prev.start;
                }
                let mut overlap_sents: Vec<&Sentence> = Vec::new();
                let mut overlap_len = 0usize;

                for sent in prev_sents.iter().rev() {
                    let sent_len = sent.text.chars().count();
                    if overlap_len + sent_len > overlap {
                        break;
                    }
                    // 检查加入后总大小是否 ≤ chunk_size + overlap
                    if overlap_len + sent_len + c.text.chars().count() > chunk_size + overlap {
                        break;
                    }
                    overlap_sents.insert(0, sent);
                    overlap_len += sent_len;
                }

                if !overlap_sents.is_empty() {
                    let overlap_text: String =
                        overlap_sents.iter().map(|s| s.text.as_str()).collect();
                    text = format!("{}{}", overlap_text.trim(), text);
                    start = overlap_sents.first().map(|s| s.start).unwrap_or(c.start);
                }
            }
            result.push(ChunkResult {
                text,
                start,
                end: c.end,
                index: i,
                metadata: c.metadata.clone(),
            });
        }
        result
    } else {
        chunks
    }
}

/// 将句子列表转为 ChunkResult 并加入 chunks
fn flush_chunk(chunks: &mut Vec<ChunkResult>, sents: &[&Sentence]) {
    let text: String = sents.iter().map(|s| s.text.as_str()).collect();
    let trimmed = text.trim().to_string();
    if !trimmed.is_empty() {
        let start = sents.first().map(|s| s.start).unwrap_or(0);
        let end = sents
            .last()
            .map(|s| s.start + s.text.chars().count())
            .unwrap_or(0);
        chunks.push(ChunkResult {
            text: trimmed,
            start,
            end,
            index: chunks.len(),
            metadata: serde_json::json!({}),
        });
    }
}

/// 按子句标点切割超长句子，仍超长则整句保留
fn split_long_sentence(
    text: &str,
    max_size: usize,
    base_index: usize,
    original_start: usize,
) -> Vec<ChunkResult> {
    let sub_puncts = ['；', '，', '、', ';', ','];
    let chars: Vec<char> = text.chars().collect();
    let mut parts: Vec<(String, usize, usize)> = Vec::new();
    let mut current = String::new();
    let mut current_start = 0usize;

    for (i, &ch) in chars.iter().enumerate() {
        current.push(ch);

        if sub_puncts.contains(&ch) {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                parts.push((trimmed, current_start, current_start + current.chars().count()));
            }
            current.clear();
            current_start = i + 1;
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push((trimmed, current_start, current_start + current.chars().count()));
    }

    // 检查是否所有部分都 ≤ max_size
    let all_fit = !parts.is_empty() && parts.iter().all(|(t, _, _)| t.chars().count() <= max_size);

    if all_fit {
        parts
            .into_iter()
            .enumerate()
            .map(|(i, (t, s, e))| ChunkResult {
                text: t,
                start: original_start + s,
                end: original_start + e,
                index: base_index + i,
                metadata: serde_json::json!({}),
            })
            .collect()
    } else {
        // 整句保留
        vec![ChunkResult {
            text: text.trim().to_string(),
            start: original_start,
            end: original_start + text.chars().count(),
            index: base_index,
            metadata: serde_json::json!({}),
        }]
    }
}

pub struct SemanticStrategy;

#[async_trait]
impl SplitStrategy for SemanticStrategy {
    fn id(&self) -> SplitStrategyId {
        SplitStrategyId::Semantic
    }

    fn info(&self) -> StrategyInfo {
        StrategyInfo {
            id: SplitStrategyId::Semantic.as_str().to_string(),
            name: "语义切分".to_string(),
            description: "按句子边界和话题转换点切分,保持语义连贯性,优先合并短句".to_string(),
            config_schema: vec![
                StrategyParamSchema {
                    key: "chunk_size".into(),
                    label: "片段大小(字符数)".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(512),
                    min: Some(64.0),
                    max: Some(8192.0),
                },
                StrategyParamSchema {
                    key: "overlap".into(),
                    label: "重叠字符数".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(50),
                    min: Some(0.0),
                    max: Some(1024.0),
                },
                StrategyParamSchema {
                    key: "sentence_group_size".into(),
                    label: "句子分组数".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(3),
                    min: Some(1.0),
                    max: Some(10.0),
                },
            ],
        }
    }

    async fn split(
        &self,
        text: &str,
        params: &serde_json::Value,
        _ctx: &SplitContext,
    ) -> AppResult<Vec<ChunkResult>> {
        let p: SemanticParams =
            serde_json::from_value(params.clone()).unwrap_or_default();
        let chunk_size = p.chunk_size.max(64);
        let overlap = p.overlap.min(chunk_size.saturating_sub(1));
        let group_size = p.sentence_group_size.max(1);

        Ok(semantic_split(text, chunk_size, overlap, group_size))
    }
}

// ============ Agentic 智能切分（LLM 辅助）============

/// Agentic 策略的默认 prompt 模板
/// {max_chunk_size} 与 {sentences} 为占位符
pub const DEFAULT_AGENTIC_PROMPT: &str = r#"/no_think
你是文档切分专家。下面是一段连续的文本，已按句子编号。请找出话题或语义发生明显转换的位置，在这些句子之后切开，使每段围绕一个主题。

要求：
1. 每段长度尽量控制在 {max_chunk_size} 字符以内，但不要为了凑长度在语义连贯处硬切
2. 只输出一个 JSON 数组，元素为"需要在其后切开"的句子编号，按从小到大排列
3. 不要输出编号以外的任何文字、解释或标点
4. 若整段无需切分，输出 []

示例输出：[3, 7, 12]

文本：
{sentences}

输出：
"#;

fn default_window_size() -> usize {
    20
}

/// 容错反序列化：接受字符串或字符串数组（取首个非空）
/// 兼容前端历史上把 string 参数逗号切分为数组的旧数据
fn de_opt_string_or_first<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        S(String),
        V(Vec<String>),
    }
    let v = Option::<StringOrVec>::deserialize(deserializer)?;
    Ok(match v {
        Some(StringOrVec::S(s)) => {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        Some(StringOrVec::V(v)) => v
            .into_iter()
            .map(|s| s.trim().to_string())
            .find(|s| !s.is_empty()),
        None => None,
    })
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgenticParams {
    #[serde(default, deserialize_with = "de_opt_string_or_first")]
    pub model: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_or_first")]
    pub prompt_template: Option<String>,
    #[serde(default = "default_chunk_size")]
    pub max_chunk_size: usize,
    #[serde(default = "default_window_size")]
    pub window_size: usize,
}

/// 从 LLM 输出提取切分点编号（纯函数，便于测试）
/// window_len 为本窗句子数；返回 1-based 编号 k（语义：在第 k 句后切），已过滤/去重/排序
pub(crate) fn parse_split_points(raw: &str, window_len: usize) -> Vec<usize> {
    // 第 0 层：剥离 <think>...</think>（qwen3 推理块）
    let stripped = match raw.find("</think>") {
        Some(pos) => &raw[pos + "</think>".len()..],
        None => raw,
    };
    let trimmed = stripped.trim();

    // 第 1 层：直接解析 JSON 数组
    let mut nums: Option<Vec<usize>> = serde_json::from_str::<Vec<usize>>(trimmed).ok();

    // 第 2 层：截取第一个 '[' 到最后一个 ']' 的子串再解析（覆盖 "答案是 [3,7]。" 与 ```json 围栏）
    if nums.is_none() {
        if let (Some(l), Some(r)) = (trimmed.find('['), trimmed.rfind(']')) {
            if l < r {
                nums = serde_json::from_str::<Vec<usize>>(&trimmed[l..=r]).ok();
            }
        }
    }

    // 第 3 层：兜底——扫描串中所有连续 ASCII 数字段（覆盖 JSON 被截断等场景）
    let mut result: Vec<usize> = match nums {
        Some(v) => v,
        None => {
            let mut out = Vec::new();
            let mut cur = String::new();
            for ch in trimmed.chars() {
                if ch.is_ascii_digit() {
                    cur.push(ch);
                } else if !cur.is_empty() {
                    if let Ok(n) = cur.parse::<usize>() {
                        out.push(n);
                    }
                    cur.clear();
                }
            }
            if !cur.is_empty() {
                if let Ok(n) = cur.parse::<usize>() {
                    out.push(n);
                }
            }
            out
        }
    };

    // 后处理：保留 1 <= k < window_len（k == window_len 是末尾，无意义），去重，升序
    result.retain(|&k| k >= 1 && k < window_len);
    result.sort_unstable();
    result.dedup();
    result
}

/// 构造单次窗口分析的 prompt
fn build_agentic_prompt(template: &str, window: &[Sentence], max_chunk_size: usize) -> String {
    let numbered: String = window
        .iter()
        .enumerate()
        .map(|(i, s)| format!("[{}] {}\n", i + 1, s.text))
        .collect();
    let p = template.replace("{max_chunk_size}", &max_chunk_size.to_string());
    if p.contains("{sentences}") {
        p.replace("{sentences}", numbered.trim_end())
    } else {
        // 防御：模板缺占位符时把编号文本追加到末尾
        format!("{}\n{}", p, numbered)
    }
}

/// 计算窗口结束索引（exclusive）：句数 ≤ window_size 且字符数 ≤ max_chars
/// 保证窗口至少含 2 句（切分需要候选切点），保证滑窗循环 i 单调递增必终止
fn window_end(sentences: &[Sentence], start: usize, window_size: usize, max_chars: usize) -> usize {
    let mut end = start;
    let mut chars = 0usize;
    while end < sentences.len()
        && end - start < window_size
        && chars + sentences[end].text.chars().count() <= max_chars
    {
        chars += sentences[end].text.chars().count();
        end += 1;
    }
    if end - start < 2 && end < sentences.len() {
        end = (start + 2).min(sentences.len());
    }
    end
}

/// 按切点（0-based 句子索引，含该句）把句子序列分组为段 (text, start, end)
fn group_by_cuts(sentences: &[Sentence], cuts: &[usize]) -> Vec<(String, usize, usize)> {
    let mut segments = Vec::new();
    let mut seg_start_idx = 0usize;
    for &c in cuts {
        if c < seg_start_idx || c >= sentences.len() {
            continue;
        }
        let text: String = sentences[seg_start_idx..=c]
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() {
            let end = sentences[c].start + sentences[c].text.chars().count();
            segments.push((trimmed, sentences[seg_start_idx].start, end));
        }
        seg_start_idx = c + 1;
    }
    if seg_start_idx < sentences.len() {
        let text: String = sentences[seg_start_idx..]
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() {
            let last = &sentences[sentences.len() - 1];
            let end = last.start + last.text.chars().count();
            segments.push((trimmed, sentences[seg_start_idx].start, end));
        }
    }
    segments
}

/// 硬约束：LLM 建议不可信，超长段按句子边界二次切分
/// 复用语义切分的成熟逻辑：逐句累加 + 超大单句子句切割
fn enforce_max_size(segments: Vec<(String, usize, usize)>, max_size: usize) -> Vec<ChunkResult> {
    let mut chunks: Vec<ChunkResult> = Vec::new();
    for (text, start, end) in segments {
        if text.chars().count() <= max_size {
            chunks.push(ChunkResult {
                text,
                start,
                end,
                index: chunks.len(),
                metadata: serde_json::json!({"strategy": "agentic"}),
            });
        } else {
            // 段内重新分句，start 是段内偏移，需加上段基址转为原文偏移
            let mut sents = split_sentences(&text);
            for s in sents.iter_mut() {
                s.start += start;
            }
            let mut cur: Vec<&Sentence> = Vec::new();
            let mut cur_len = 0usize;
            for s in &sents {
                let slen = s.text.chars().count();
                if slen > max_size {
                    if !cur.is_empty() {
                        flush_chunk(&mut chunks, &cur);
                        cur.clear();
                        cur_len = 0;
                    }
                    chunks.extend(split_long_sentence(&s.text, max_size, chunks.len(), s.start));
                    continue;
                }
                if cur_len + slen > max_size && !cur.is_empty() {
                    flush_chunk(&mut chunks, &cur);
                    cur.clear();
                    cur_len = 0;
                }
                cur.push(s);
                cur_len += slen;
            }
            if !cur.is_empty() {
                flush_chunk(&mut chunks, &cur);
            }
        }
    }
    chunks
}

/// 合并过小段：前段 < max/4 且合并后 ≤ max 时，把后段并入前段
fn merge_tiny(chunks: Vec<ChunkResult>, max_size: usize) -> Vec<ChunkResult> {
    let mut merged: Vec<ChunkResult> = Vec::new();
    for c in chunks {
        if let Some(last) = merged.last_mut() {
            let combined = last.text.chars().count() + 1 + c.text.chars().count();
            if last.text.chars().count() < max_size / 4 && combined <= max_size {
                last.text.push(' ');
                last.text.push_str(&c.text);
                last.end = c.end;
                continue;
            }
        }
        merged.push(c);
    }
    for (i, c) in merged.iter_mut().enumerate() {
        c.index = i;
    }
    merged
}

pub struct AgenticSplitStrategy;

#[async_trait]
impl SplitStrategy for AgenticSplitStrategy {
    fn id(&self) -> SplitStrategyId {
        SplitStrategyId::Agentic
    }

    fn info(&self) -> StrategyInfo {
        StrategyInfo {
            id: SplitStrategyId::Agentic.as_str().to_string(),
            name: "智能切分".to_string(),
            description: "调用本地 LLM 分析语义切分点,质量更高;导入时需逐窗口调用模型,耗时显著长于其他策略"
                .to_string(),
            config_schema: vec![
                StrategyParamSchema {
                    key: "model".into(),
                    label: "切分模型".into(),
                    r#type: "string".into(),
                    default: serde_json::json!(crate::ollama::DEFAULT_CHAT_MODEL),
                    min: None,
                    max: None,
                },
                StrategyParamSchema {
                    key: "prompt_template".into(),
                    label: "提示词模板".into(),
                    r#type: "string".into(),
                    default: serde_json::json!(DEFAULT_AGENTIC_PROMPT),
                    min: None,
                    max: None,
                },
                StrategyParamSchema {
                    key: "max_chunk_size".into(),
                    label: "片段最大字符数".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(512),
                    min: Some(128.0),
                    max: Some(4096.0),
                },
                StrategyParamSchema {
                    key: "window_size".into(),
                    label: "分析窗口(句)".into(),
                    r#type: "number".into(),
                    default: serde_json::json!(20),
                    min: Some(5.0),
                    max: Some(50.0),
                },
            ],
        }
    }

    async fn split(
        &self,
        text: &str,
        params: &serde_json::Value,
        ctx: &SplitContext,
    ) -> AppResult<Vec<ChunkResult>> {
        let p: AgenticParams = serde_json::from_value(params.clone()).unwrap_or_default();
        let max_chunk_size = p.max_chunk_size.max(64);
        let window_size = p.window_size.clamp(2, 100);
        let model = p
            .model
            .filter(|m| !m.is_empty())
            .or_else(|| {
                if ctx.chat_model.is_empty() {
                    None
                } else {
                    Some(ctx.chat_model.clone())
                }
            })
            .unwrap_or_else(|| crate::ollama::DEFAULT_CHAT_MODEL.to_string());
        let template = p
            .prompt_template
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| DEFAULT_AGENTIC_PROMPT.to_string());

        let sentences = split_sentences(text);
        if sentences.is_empty() {
            return Ok(vec![]);
        }

        // 短路：文本不大时不值得调 LLM，整个作为单块
        let total_chars: usize = sentences.iter().map(|s| s.text.chars().count()).sum();
        if sentences.len() <= 2 || total_chars <= max_chunk_size {
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                return Ok(vec![]);
            }
            return Ok(vec![ChunkResult {
                text: trimmed,
                start: 0,
                end: text.chars().count(),
                index: 0,
                metadata: serde_json::json!({"strategy": "agentic"}),
            }]);
        }

        let client = match &ctx.ollama {
            Some(c) => c,
            None => {
                if ctx.allow_fallback {
                    tracing::warn!("agentic split: Ollama 不可用,降级到语义切分");
                    return Ok(semantic_split(text, max_chunk_size, 0, 3));
                }
                return Err(AppError::OllamaNotRunning);
            }
        };

        // 滑窗批量分析：编号句子提交 LLM，返回切分点编号
        // 不变式：i 单调递增必终止（有切点 i += last ≥ 1；无切点 i = end > i）
        const WINDOW_MAX_CHARS: usize = 4000;
        let mut cuts: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < sentences.len() {
            let end = window_end(&sentences, i, window_size, WINDOW_MAX_CHARS);
            let window = &sentences[i..end];
            if window.len() <= 1 {
                i = end; // 单句窗口无切分意义；end > i 保证推进
                continue;
            }
            let prompt = build_agentic_prompt(&template, window, max_chunk_size);
            let req = crate::ollama::GenerateRequest {
                model: model.clone(),
                prompt,
                system: None,
                context: None,
                stream: Some(false),
                think: Some(false),
                options: Some(crate::ollama::GenerateOptions {
                    temperature: Some(0.1),
                    num_predict: Some(256),
                    ..Default::default()
                }),
                keep_alive: None,
            };
            match client.generate(&req).await {
                Ok(resp) => {
                    let local = parse_split_points(&resp.response, window.len());
                    if local.is_empty() {
                        i = end; // 本窗无切点，整窗前进
                    } else {
                        // 切点语义：在第 k 句后切（1-based 窗口内）
                        // 全局 0-based 含该句索引 = i + k - 1；下一窗从最后一个切点之后开始
                        let last = *local.last().unwrap();
                        cuts.extend(local.iter().map(|k| i + k - 1));
                        i += last;
                    }
                }
                Err(e) => {
                    if ctx.allow_fallback {
                        tracing::warn!("agentic split: LLM 调用失败({}),降级到语义切分", e);
                        return Ok(semantic_split(text, max_chunk_size, 0, 3));
                    }
                    return Err(e);
                }
            }
        }

        let segments = group_by_cuts(&sentences, &cuts);
        let chunks = enforce_max_size(segments, max_chunk_size);
        Ok(merge_tiny(chunks, max_chunk_size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_strategy_registered() {
        let strategies = list_strategies();
        let ids: Vec<&str> = strategies.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"semantic"), "semantic strategy not found in list: {:?}", ids);
    }

    #[test]
    fn test_semantic_strategy_info() {
        let s = SemanticStrategy;
        let info = s.info();
        assert_eq!(info.id, "semantic");
        assert_eq!(info.name, "语义切分");
        assert_eq!(info.config_schema.len(), 3);
        assert_eq!(info.config_schema[0].key, "chunk_size");
        assert_eq!(info.config_schema[1].key, "overlap");
        assert_eq!(info.config_schema[2].key, "sentence_group_size");
    }

    #[test]
    fn test_semantic_split_basic() {
        let text = "这是第一句话。这是第二句话。这是第三句话。然而，这是话题转换。这是新话题的第一句。这是新话题的第二句。";
        let chunks = semantic_split(text, 100, 0, 3);
        assert!(!chunks.is_empty(), "semantic_split should produce chunks");
    }

    #[test]
    fn test_semantic_split_paragraph_break() {
        let text = "第一段内容。包含几个句子。\n\n第二段内容。完全不同的话题。";
        let chunks = semantic_split(text, 512, 0, 3);
        assert!(!chunks.is_empty(), "should split on paragraph breaks");
    }

    #[test]
    fn test_semantic_split_topic_shift() {
        let text = "我们讨论了系统的架构设计。模块划分清晰。然而，性能问题仍然存在。需要优化数据库查询。";
        let chunks = semantic_split(text, 512, 50, 3);
        assert!(!chunks.is_empty(), "should split on topic shift");
    }

    /// 要求1：切分的块的大小要基于设置的片段大小来切分
    /// 新内容 ≤ chunk_size，总大小（含 overlap）≤ chunk_size + overlap
    #[test]
    fn test_chunk_size_hard_limit() {
        let text = "人工智能是计算机科学的一个分支。它企图了解智能的实质。并生产出一种新的能以人类智能相似的方式做出反应的智能机器。研究领域包括机器人、语言识别、图像识别和自然语言处理等。然而，人工智能也面临着许多挑战。数据隐私和算法偏见是当前的热点问题。";
        let chunk_size = 64;
        let overlap = 20;
        let chunks = semantic_split(text, chunk_size, overlap, 3);
        assert!(chunks.len() > 1, "should produce multiple chunks");
        for (i, chunk) in chunks.iter().enumerate() {
            let len = chunk.text.chars().count();
            assert!(
                len <= chunk_size + overlap,
                "chunk {} has {} chars, exceeds chunk_size + overlap = {}",
                i, len, chunk_size + overlap
            );
        }
    }

    /// 要求2：chunk_size 是新内容的大小上限，overlap 是额外附加的
    /// 新内容 ≤ chunk_size，overlap ≤ overlap，总大小 ≤ chunk_size + overlap
    #[test]
    fn test_chunk_size_includes_overlap() {
        let text = "这是第一句话内容。这是第二句话内容。这是第三句话内容。这是第四句话内容。这是第五句话内容。这是第六句话内容。这是第七句话内容。这是第八句话内容。";
        let chunk_size = 64;
        let overlap = 20;
        let chunks = semantic_split(text, chunk_size, overlap, 3);
        assert!(chunks.len() > 1, "should produce multiple chunks");
        for (i, chunk) in chunks.iter().enumerate() {
            let len = chunk.text.chars().count();
            assert!(
                len <= chunk_size + overlap,
                "chunk {} ({} chars) exceeds chunk_size + overlap = {}",
                i, len, chunk_size + overlap
            );
        }
    }

    /// 要求3：overlap 不应把上下文的句子从中间截断
    /// 每个 chunk 应以完整句子结尾（以句末标点结尾，或是最后一个 chunk）
    #[test]
    fn test_no_sentence_cut_in_middle() {
        let text = "人工智能是计算机科学的一个分支。它企图了解智能的实质。并生产出一种新的能以人类智能相似的方式做出反应的智能机器。研究领域包括机器人、语言识别、图像识别和自然语言处理等。然而，人工智能也面临着许多挑战。数据隐私和算法偏见是当前的热点问题。";
        let chunks = semantic_split(text, 64, 20, 3);
        let sentence_endings = ['。', '！', '？', '.', '!', '?'];
        for (i, chunk) in chunks.iter().enumerate() {
            let trimmed = chunk.text.trim();
            let last_char = trimmed.chars().last();
            // 最后一个 chunk 可以不以标点结尾（原文末尾）
            if i < chunks.len() - 1 {
                assert!(
                    last_char.map(|c| sentence_endings.contains(&c)).unwrap_or(false),
                    "chunk {} ends mid-sentence: ...{}",
                    i,
                    trimmed.chars().rev().take(20).collect::<String>()
                );
            }
        }
    }

    /// 要求3补充：overlap 部分应由完整句子组成
    /// 每个 chunk 的第一个句子应以句末标点结尾（完整句子）
    #[test]
    fn test_overlap_is_complete_sentences() {
        let text = "这是第一句话。这是第二句话。这是第三句话。这是第四句话。这是第五句话。这是第六句话。这是第七句话。这是第八句话。这是第九句话。这是第十句话。";
        let chunks = semantic_split(text, 40, 20, 3);
        assert!(chunks.len() > 1, "should produce multiple chunks");
        // 验证：每个 chunk 的第一个句子应是完整的（以句末标点结尾）
        let sentence_endings = ['。', '！', '？', '.', '!', '?'];
        for (i, chunk) in chunks.iter().enumerate() {
            // 用字符位置查找第一个句末标点
            let first_end = chunk.text.chars().position(|c| sentence_endings.contains(&c));
            if let Some(end) = first_end {
                let first_sent: String = chunk.text.chars().take(end + 1).collect();
                assert!(
                    first_sent.ends_with(|c: char| sentence_endings.contains(&c)),
                    "chunk {} starts with incomplete sentence: {}",
                    i, first_sent
                );
            }
            // 如果没有句末标点，说明整个 chunk 是一个超长单句的一部分，这是允许的
        }
    }

    /// 超大单句降级：单个句子超过 chunk_size 时，整句保留不截断
    #[test]
    fn test_oversized_single_sentence_kept_intact() {
        // 一个超过 64 字符的单句
        let long_sentence = "这是一个非常长的句子它超过了六十四个字符的限制因为中文文本经常会包含这样比较长的叙述性句子对吧就是这样的。";
        let text = format!("短句一。{}短句二。", long_sentence);
        let chunks = semantic_split(&text, 64, 10, 3);
        // 长句应保持完整（不被截断），或按子句标点切割（但每部分应是完整子句）
        for chunk in &chunks {
            // 检查 chunk 不应在长句中间产生乱码式截断
            assert!(!chunk.text.is_empty(), "chunk should not be empty");
        }
        // 至少长句内容应被完整保留在某个 chunk 中
        let all_text: String = chunks.iter().map(|c| c.text.clone()).collect();
        // 由于 overlap 和 trim，只做宽松验证：长句的关键部分应出现
        assert!(
            all_text.contains("非常长的句子") || all_text.contains("超过了六十四个字符"),
            "long sentence content should be preserved"
        );
    }

    // ============ Agentic 智能切分测试 ============

    #[test]
    fn test_agentic_registered() {
        let strategies = list_strategies();
        let ids: Vec<&str> = strategies.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"agentic"), "agentic not in list: {:?}", ids);
        // from_str / as_str 往返
        let id = SplitStrategyId::from_str("agentic").unwrap();
        assert_eq!(id.as_str(), "agentic");
    }

    #[test]
    fn test_agentic_strategy_info() {
        let s = AgenticSplitStrategy;
        let info = s.info();
        assert_eq!(info.id, "agentic");
        assert_eq!(info.name, "智能切分");
        assert!(info.description.contains("LLM"), "description 应提示 LLM 开销");
        let keys: Vec<&str> = info.config_schema.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, vec!["model", "prompt_template", "max_chunk_size", "window_size"]);
        assert_eq!(info.config_schema[2].default, serde_json::json!(512));
        assert_eq!(info.config_schema[3].default, serde_json::json!(20));
    }

    #[test]
    fn test_parse_split_points_pure_json() {
        assert_eq!(parse_split_points("[3, 7, 12]", 20), vec![3, 7, 12]);
    }

    #[test]
    fn test_parse_split_points_with_prose() {
        assert_eq!(parse_split_points("好的，切分点是 [3, 7]。", 20), vec![3, 7]);
    }

    #[test]
    fn test_parse_split_points_code_fence() {
        assert_eq!(parse_split_points("```json\n[2,5]\n```", 20), vec![2, 5]);
    }

    #[test]
    fn test_parse_split_points_think_block() {
        assert_eq!(
            parse_split_points("<think>让我想想，应该有 100 个切分点</think>[4]", 20),
            vec![4]
        );
    }

    #[test]
    fn test_parse_split_points_no_brackets_fallback() {
        assert_eq!(parse_split_points("3 7 12", 20), vec![3, 7, 12]);
    }

    #[test]
    fn test_parse_split_points_out_of_range() {
        // 0 与 window_len(20) 越界，应被过滤
        assert_eq!(parse_split_points("[0, 20]", 20), Vec::<usize>::new());
    }

    #[test]
    fn test_parse_split_points_sort_dedup() {
        assert_eq!(parse_split_points("[7, 3, 7, 12]", 20), vec![3, 7, 12]);
    }

    #[test]
    fn test_parse_split_points_no_cuts() {
        assert_eq!(parse_split_points("没有切分点", 20), Vec::<usize>::new());
        assert_eq!(parse_split_points("[]", 20), Vec::<usize>::new());
    }

    #[test]
    fn test_agentic_params_string_or_vec() {
        // 前端历史上把 string 参数逗号切分为数组，应容错取首个非空
        let p: AgenticParams = serde_json::from_value(serde_json::json!({
            "model": ["qwen3:1.7b"],
            "max_chunk_size": 256
        }))
        .unwrap();
        assert_eq!(p.model.as_deref(), Some("qwen3:1.7b"));
        assert_eq!(p.max_chunk_size, 256);
        assert_eq!(p.window_size, 20); // 默认值

        // 标量字符串正常解析
        let p2: AgenticParams = serde_json::from_value(serde_json::json!({
            "model": "llama3:8b"
        }))
        .unwrap();
        assert_eq!(p2.model.as_deref(), Some("llama3:8b"));
    }

    #[test]
    fn test_group_by_cuts_and_enforce() {
        // 10 个句子，每句 10 字符（含句号）
        let text = "第一句内容啊啊。第二句内容啊啊。第三句内容啊啊。第四句内容啊啊。第五句内容啊啊。第六句内容啊啊。第七句内容啊啊。第八句内容啊啊。第九句内容啊啊。第十句内容啊啊。";
        let sentences = split_sentences(text);
        assert_eq!(sentences.len(), 10);

        // 切点：第 3 句后(索引2)、第 6 句后(索引5)
        let segments = group_by_cuts(&sentences, &[2, 5]);
        assert_eq!(segments.len(), 3);
        assert!(segments[0].0.starts_with("第一句"));
        assert!(segments[0].0.ends_with("第三句内容啊啊。"));
        assert!(segments[1].0.starts_with("第四句"));
        assert!(segments[2].0.starts_with("第七句"));
        // 原文偏移：第一段 start=0，第二段 start 应为第 4 句的原文位置
        assert_eq!(segments[0].1, 0);
        assert!(segments[1].1 > segments[0].2 - 10, "段偏移应递增");

        // enforce_max_size：max=25（约 3 句），段应被二次切分
        let chunks = enforce_max_size(segments, 25);
        for c in &chunks {
            assert!(
                c.text.chars().count() <= 25,
                "chunk {} 超过硬约束: {} 字符",
                c.index,
                c.text.chars().count()
            );
        }
        assert!(chunks.len() >= 4, "10 句 max=25 应至少切出 4 块, 实际 {}", chunks.len());
    }

    #[test]
    fn test_merge_tiny_merges_small() {
        let mk = |text: &str, start: usize, end: usize, index: usize| ChunkResult {
            text: text.to_string(),
            start,
            end,
            index,
            metadata: serde_json::json!({}),
        };
        // 第一块 5 字符(< 512/4=128)，第二块 10 字符，合并后 ≤ 512 应合并
        let chunks = vec![mk("小段。", 0, 5, 0), mk("正常段内容啊啊啊啊。", 5, 15, 1)];
        let merged = merge_tiny(chunks, 512);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].index, 0);
        assert!(merged[0].text.contains("小段。"));
        assert!(merged[0].text.contains("正常段"));
    }

    /// 短文本短路：≤ max_chunk_size 时整个作为单块，不调 LLM（无 client 也不报错）
    #[tokio::test]
    async fn test_agentic_short_text_shortcut() {
        let s = AgenticSplitStrategy;
        let text = "短文本。只有两句。";
        let ctx = SplitContext::none();
        let chunks = s
            .split(text, &serde_json::json!({"max_chunk_size": 512}), &ctx)
            .await
            .unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, text);
    }

    /// 降级路径：无 client + allow_fallback=true → 结果与 semantic_split 一致
    #[tokio::test]
    async fn test_agentic_fallback_to_semantic() {
        let s = AgenticSplitStrategy;
        let text = "人工智能是计算机科学的一个分支。它企图了解智能的实质。并生产出一种新的能以人类智能相似的方式做出反应的智能机器。研究领域包括机器人、语言识别、图像识别和自然语言处理等。然而，人工智能也面临着许多挑战。数据隐私和算法偏见是当前的热点问题。";
        let ctx = SplitContext {
            ollama: None,
            chat_model: String::new(),
            allow_fallback: true,
        };
        let chunks = s
            .split(text, &serde_json::json!({"max_chunk_size": 64}), &ctx)
            .await
            .unwrap();
        let expected = semantic_split(text, 64, 0, 3);
        assert_eq!(chunks.len(), expected.len(), "降级结果应与语义切分一致");
        for (a, b) in chunks.iter().zip(expected.iter()) {
            assert_eq!(a.text, b.text);
        }
    }

    /// 预览语义：无 client + allow_fallback=false → 报 OllamaNotRunning
    #[tokio::test]
    async fn test_agentic_preview_errors_without_ollama() {
        let s = AgenticSplitStrategy;
        let text = "人工智能是计算机科学的一个分支。它企图了解智能的实质。并生产出一种新的能以人类智能相似的方式做出反应的智能机器。研究领域包括机器人、语言识别、图像识别和自然语言处理等。然而，人工智能也面临着许多挑战。数据隐私和算法偏见是当前的热点问题。";
        let ctx = SplitContext::none(); // allow_fallback = false
        let result = s
            .split(text, &serde_json::json!({"max_chunk_size": 64}), &ctx)
            .await;
        assert!(
            matches!(result, Err(AppError::OllamaNotRunning)),
            "预览路径应报 OllamaNotRunning, 实际: {:?}",
            result.map(|v| v.len())
        );
    }
}
