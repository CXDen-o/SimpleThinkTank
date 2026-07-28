// 贴底跟随 composable(通用聊天/日志列表滚动方案)
// 非对称判定(对齐 Vercel use-stick-to-bottom 主流方案):
//   逃逸:看用户意图——wheel 上滚/触摸下滑/键盘上翻/滚动条上拖,输入事件同步生效,与流式 token 无竞态
//   回粘:看方向+位置——仅"向下滚动且距底 ≤ BOTTOM_THRESHOLD"才恢复跟随;
//        向上滚动一律脱离(无论距底多近),位置不变不动状态——回粘永远不会覆盖一次上翻
// 跟随滚动用 rAF 合帧:每帧最多写一次 scrollTop,避免高频 token 下的布局抖动
// 方案变迁详见 docs/chat-stick-to-bottom.md

import { ref, nextTick, onMounted, onUnmounted, type Ref } from "vue";

/** 回粘判定阈值(px):向下滚动到距底小于该值视为"回到底部",恢复跟随 */
const BOTTOM_THRESHOLD = 100;

/** 滚动方向死区(px):小于该值的位置抖动视为"未移动"(亚像素/布局微调) */
const DIRECTION_DEADZONE = 1;

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

  /** 跟随滚动的 rAF 合帧句柄(流式 token 高频下每帧最多滚一次) */
  let followRafId: number | null = null;

  /** 上一次 scrollTop,用于滚动方向判定(滚动条拖拽无 wheel/touch 事件,只能靠方向识别) */
  let lastScrollTop = 0;

  /** 触摸起点 Y,用于触摸滑动方向判定 */
  let lastTouchY = 0;

  function distanceToBottom(el: HTMLElement): number {
    return el.scrollHeight - el.scrollTop - el.clientHeight;
  }

  /** 用户明确的"上翻"意图:立即脱离跟随(输入事件同步触发,不存在与 token 的竞态) */
  function escapeFollow() {
    if (animating) cancelJump();
    stickToBottom.value = false;
  }

  function onScroll() {
    // 动画期间 scrollTop 由 rAF 驱动,不回写状态
    if (animating) return;
    const el = containerRef.value;
    if (!el) return;
    const delta = el.scrollTop - lastScrollTop;
    lastScrollTop = el.scrollTop;
    if (delta < -DIRECTION_DEADZONE) {
      // 向上移动:用户上翻(滚动条拖拽/惯性滚动等无输入事件的渠道),脱离跟随
      // ——无论距底多近。回粘判定绝不能在用户上滚途中生效,否则逃逸会被立即覆盖
      stickToBottom.value = false;
    } else if (
      delta > DIRECTION_DEADZONE &&
      distanceToBottom(el) <= BOTTOM_THRESHOLD
    ) {
      // 向下移动且回到距底阈值内:恢复跟随(回粘阈值放宽,降低回底难度)
      stickToBottom.value = true;
    }
    // 位置基本不变(内容增长/布局调整):保持现状,不改动跟随状态
  }

  function cancelJump() {
    if (rafId !== null) cancelAnimationFrame(rafId);
    rafId = null;
    animating = false;
  }

  function onWheel(e: WheelEvent) {
    if (animating) {
      // 动画期间用户主动滚动:中断动画,尊重用户操作
      cancelJump();
      onScroll();
      return;
    }
    // deltaY < 0:上滚,立即脱离跟随(不等 scroll 事件,消除与流式 token 的竞态)
    if (e.deltaY < 0) escapeFollow();
    // deltaY > 0:下滚不处理,由 scroll 事件在接近底部时恢复跟随
  }

  function onTouchStart(e: TouchEvent) {
    lastTouchY = e.touches[0].clientY;
  }

  function onTouchMove(e: TouchEvent) {
    if (animating) {
      cancelJump();
      onScroll();
      return;
    }
    const y = e.touches[0].clientY;
    // 手指下滑(y 增大)= 内容上翻 → 脱离跟随
    if (y > lastTouchY) escapeFollow();
    lastTouchY = y;
  }

  /** 键盘滚动(PageUp/Home/↑ 上翻;End 回底) */
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "PageUp" || e.key === "Home" || e.key === "ArrowUp") {
      escapeFollow();
    } else if (e.key === "End") {
      stickToBottom.value = true;
    }
  }

  /** 强制滚动到底部(即时定位)并重置跟随状态 */
  async function scrollToBottom() {
    await nextTick();
    const el = containerRef.value;
    if (!el) return;
    // 即时定位:流式高频(数十 ms/token)下 smooth 动画会累积卡顿
    el.scrollTop = el.scrollHeight;
    lastScrollTop = el.scrollTop;
    stickToBottom.value = true;
  }

  /** 内容变化时调用:贴底跟随中则调度一次 rAF 滚动(同帧多次调用合并) */
  function followIfStuck() {
    if (animating || !stickToBottom.value) return;
    if (followRafId !== null) return; // 本帧已排队
    followRafId = requestAnimationFrame(() => {
      followRafId = null;
      const el = containerRef.value;
      // 帧执行前复查:本帧内用户可能已发起上翻逃逸
      if (!el || animating || !stickToBottom.value) return;
      el.scrollTop = el.scrollHeight;
      lastScrollTop = el.scrollTop;
    });
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
      lastScrollTop = cur.scrollTop;
      if (t < 1) {
        rafId = requestAnimationFrame(step);
      } else {
        cur.scrollTop = cur.scrollHeight; // 终值对齐
        lastScrollTop = cur.scrollTop;
        cancelJump();
        stickToBottom.value = true;
      }
    };
    rafId = requestAnimationFrame(step);
  }

  onMounted(() => {
    const el = containerRef.value;
    el?.addEventListener("scroll", onScroll, { passive: true });
    el?.addEventListener("wheel", onWheel, { passive: true });
    el?.addEventListener("touchstart", onTouchStart, { passive: true });
    el?.addEventListener("touchmove", onTouchMove, { passive: true });
    el?.addEventListener("keydown", onKeydown);
  });

  onUnmounted(() => {
    cancelJump();
    if (followRafId !== null) cancelAnimationFrame(followRafId);
    followRafId = null;
    const el = containerRef.value;
    el?.removeEventListener("scroll", onScroll);
    el?.removeEventListener("wheel", onWheel);
    el?.removeEventListener("touchstart", onTouchStart);
    el?.removeEventListener("touchmove", onTouchMove);
    el?.removeEventListener("keydown", onKeydown);
  });

  return {
    stickToBottom,
    followIfStuck,
    scrollToBottom,
    jumpToLatest,
  };
}
