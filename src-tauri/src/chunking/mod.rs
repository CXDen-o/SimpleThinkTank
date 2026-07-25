// 文本切分模块
// 实现 TRD 3.3 切分策略配置:固定大小 / 递归字符 / 基于结构

pub mod strategies;
pub mod commands;

pub use strategies::{SplitContext, SplitStrategyId, get_strategy};
