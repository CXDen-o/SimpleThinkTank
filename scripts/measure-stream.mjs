// 流式输出实测脚本:测量 Ollama /api/generate 的 token 到达节奏
// 用法: node scripts/measure-stream.mjs
const t0 = Date.now();
const resp = await fetch("http://localhost:11434/api/generate", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    model: "qwen3:1.7b",
    prompt: "用三句话介绍北京的历史。",
    stream: true,
    think: false,
    options: { temperature: 0.7, num_predict: 256 },
  }),
});

const reader = resp.body.getReader();
const decoder = new TextDecoder();
let buf = "";
let firstTokenAt = null;
let firstThinkAt = null;
let thinkCount = 0;
let last = t0;
let count = 0;
const intervals = [];

while (true) {
  const { done, value } = await reader.read();
  if (done) break;
  buf += decoder.decode(value, { stream: true });
  let idx;
  while ((idx = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, idx).trim();
    buf = buf.slice(idx + 1);
    if (!line) continue;
    const j = JSON.parse(line);
    const now = Date.now();
    if (j.thinking) {
      thinkCount++;
      if (firstThinkAt === null) {
        firstThinkAt = now - t0;
        console.log(`[首个 thinking chunk] +${firstThinkAt}ms  "${j.thinking.slice(0, 40)}..."`);
      }
    }
    if (j.response) {
      count++;
      if (firstTokenAt === null) {
        firstTokenAt = now - t0;
        console.log(`[首 response token] +${firstTokenAt}ms  "${j.response}"`);
      } else {
        intervals.push(now - last);
        if (count <= 10) console.log(`  +${now - last}ms  "${j.response}"`);
      }
      last = now;
    }
    if (j.done) {
      const total = now - t0;
      const evalCount = j.eval_count ?? 0;
      const evalSecs = (j.eval_duration ?? 0) / 1e9;
      const prefillMs = ((j.prompt_eval_duration ?? 0) / 1e6).toFixed(0);
      console.log("\n===== 汇总 =====");
      console.log(`thinking chunk 数: ${thinkCount}(前端不可见)`);
      console.log(`response token 事件数: ${count}(前端可见)`);
      console.log(`首 response token 延迟: ${firstTokenAt}ms`);
      console.log(`总耗时: ${total}ms`);
      console.log(`Ollama 统计: prefill=${prefillMs}ms, 生成=${evalSecs.toFixed(2)}s, ${evalCount} tokens(含 thinking), 速率=${(evalCount / evalSecs).toFixed(1)} tok/s`);
      if (intervals.length) {
        const sorted = [...intervals].sort((a, b) => a - b);
        const avg = intervals.reduce((a, b) => a + b, 0) / intervals.length;
        const p50 = sorted[Math.floor(sorted.length / 2)];
        const p95 = sorted[Math.floor(sorted.length * 0.95)];
        console.log(`response token 间隔: 平均=${avg.toFixed(1)}ms, p50=${p50}ms, p95=${p95}ms, 最大=${sorted[sorted.length - 1]}ms`);
      }
    }
  }
}
