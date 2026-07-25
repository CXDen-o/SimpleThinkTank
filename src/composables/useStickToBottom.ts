// 贴底跟随 composable(通用聊天/日志列表滚动方案)
// 状态机:stickToBottom=true 时内容增长自动跟随滚动;
// 用户上翻(距底超过阈值)后停止跟随,回到底部或调 jumpToLatest 恢复
// 详见 docs/chat-stick-to-bottom.md

import { ref, nextTick, onMounted, onUnmounted, type Ref } from "vue";

/** 距底判定阈值(px,容忍亚像素/边框/滚动条误差) */
const BOTTOM_THRESHOLD = 40;

/** "回到最新"动画时长(ms) */
const JUMP_ANIMATION_MS = 300;

/** easeOutCubic 缓动:起步快、收尾缓 */
const easeOut = (t: number) => 1 - Math.pow(1 - t, 3);

export function useStickToBottom(containerRef: Ref<HTMLElement | undefined>) {
  /** 是否贴底跟随中(初始 true:打开页面即从底部看起) */
  const stickToBottom = ref(true);

  // ---- 回底动画状态(非响应式,仅闭包内使用) ----
  // 不用原生 behavior:"smooth":其目标位置在调用时固定,流式增长中动画结束即过期;
  // 且动画期间的 scroll 事件与 auto 跟随定位都会干扰/打断它。
  // rAF 每帧重算最新目标,流式输出中也能追到真正的底部。
  let animating = false;
  let rafId: number | null = null;

  function distanceToBottom(el: HTMLElement): number {
    return el.scrollHeight - el.scrollTop - el.clientHeight;
  }

  function onScroll() {
    // 动画期间 scrollTop 由 rAF 驱动,不回写状态
    if (animating) return;
    const el = containerRef.value;
    if (!el) return;
    stickToBottom.value = distanceToBottom(el) <= BOTTOM_THRESHOLD;
  }

  function cancelJump() {
    if (rafId !== null) cancelAnimationFrame(rafId);
    rafId = null;
    animating = false;
  }

  /** 用户在动画期间主动滚动(滚轮/触摸):中断动画,尊重用户操作 */
  function onUserScrollIntent() {
    if (!animating) return;
    cancelJump();
    onScroll(); // 按当前位置立即重新判定
  }

  /** 强制滚动到底部(即时定位)并重置跟随状态 */
  async function scrollToBottom() {
    await nextTick();
    const el = containerRef.value;
    if (!el) return;
    // 即时定位:流式高频(数十 ms/token)下 smooth 动画会累积卡顿
    el.scrollTop = el.scrollHeight;
    stickToBottom.value = true;
  }

  /** 内容变化时调用:仅贴底跟随中才滚动;回底动画自身追底,不干扰 */
  async function followIfStuck() {
    if (animating) return;
    if (stickToBottom.value) {
      await scrollToBottom();
    }
  }

  /** "回到最新"按钮点击:rAF 平滑动画回底并恢复跟随 */
  async function jumpToLatest() {
    await nextTick();
    const el = containerRef.value;
    if (!el) return;
    cancelJump();
    animating = true;
    // 立即隐藏按钮并声明跟随意图;动画结束于底部后状态自然成立
    stickToBottom.value = true;

    const start = el.scrollTop;
    const startTime = performance.now();

    const step = (now: number) => {
      const cur = containerRef.value;
      if (!cur) {
        cancelJump();
        return;
      }
      const t = Math.min(1, (now - startTime) / JUMP_ANIMATION_MS);
      // 每帧取最新目标:流式增长中也能追到真正的底部
      const target = cur.scrollHeight - cur.clientHeight;
      cur.scrollTop = start + (target - start) * easeOut(t);
      if (t < 1) {
        rafId = requestAnimationFrame(step);
      } else {
        cur.scrollTop = cur.scrollHeight; // 终值对齐
        cancelJump();
        stickToBottom.value = true;
      }
    };
    rafId = requestAnimationFrame(step);
  }

  onMounted(() => {
    const el = containerRef.value;
    el?.addEventListener("scroll", onScroll, { passive: true });
    el?.addEventListener("wheel", onUserScrollIntent, { passive: true });
    el?.addEventListener("touchmove", onUserScrollIntent, { passive: true });
  });

  onUnmounted(() => {
    cancelJump();
    const el = containerRef.value;
    el?.removeEventListener("scroll", onScroll);
    el?.removeEventListener("wheel", onUserScrollIntent);
    el?.removeEventListener("touchmove", onUserScrollIntent);
  });

  return {
    stickToBottom,
    followIfStuck,
    scrollToBottom,
    jumpToLatest,
  };
}
