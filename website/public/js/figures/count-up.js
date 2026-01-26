/**
 * Count-Up Animation Module
 * Animated number counters with tabular-nums for width stability
 * Works with DataStar signals for reactive updates
 */

// Animation configuration
const DEFAULT_DURATION = 1000; // ms
const DEFAULT_EASING = 'easeOutExpo';

// Easing functions
const easings = {
  linear: t => t,
  easeOutQuad: t => t * (2 - t),
  easeOutCubic: t => (--t) * t * t + 1,
  easeOutExpo: t => t === 1 ? 1 : 1 - Math.pow(2, -10 * t),
  easeInOutCubic: t => t < 0.5 ? 4 * t * t * t : (t - 1) * (2 * t - 2) * (2 * t - 2) + 1,
};

/**
 * Animate a number from start to end
 * @param {HTMLElement} element - Element to update
 * @param {number} start - Starting value
 * @param {number} end - Ending value
 * @param {Object} options - Animation options
 */
export function countUp(element, start, end, options = {}) {
  const {
    duration = DEFAULT_DURATION,
    easing = DEFAULT_EASING,
    formatter = (n) => n.toLocaleString(),
    onComplete = null,
  } = options;

  const easingFn = typeof easing === 'function' ? easing : easings[easing] || easings.linear;
  const startTime = performance.now();
  const range = end - start;

  function update(currentTime) {
    const elapsed = currentTime - startTime;
    const progress = Math.min(elapsed / duration, 1);
    const easedProgress = easingFn(progress);
    const currentValue = start + (range * easedProgress);

    element.textContent = formatter(Math.round(currentValue));

    if (progress < 1) {
      requestAnimationFrame(update);
    } else {
      element.textContent = formatter(end);
      if (onComplete) onComplete();
    }
  }

  requestAnimationFrame(update);
}

/**
 * Initialize count-up elements with data-countup attribute
 * Usage: <span data-countup="1000" data-countup-duration="2000">0</span>
 */
export function initCountUpElements() {
  const elements = document.querySelectorAll('[data-countup]');

  elements.forEach(el => {
    const endValue = parseInt(el.dataset.countup, 10);
    const startValue = parseInt(el.dataset.countupStart || '0', 10);
    const duration = parseInt(el.dataset.countupDuration || DEFAULT_DURATION, 10);
    const trigger = el.dataset.countupTrigger || 'visible';

    if (trigger === 'visible') {
      // Use Intersection Observer for visibility-triggered animation
      const observer = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
          if (entry.isIntersecting) {
            countUp(el, startValue, endValue, { duration });
            observer.unobserve(el);
          }
        });
      }, { threshold: 0.5 });

      observer.observe(el);
    } else if (trigger === 'immediate') {
      countUp(el, startValue, endValue, { duration });
    }
  });
}

/**
 * Create a counter that responds to DataStar signal changes
 * @param {HTMLElement} element - Counter element
 * @param {Object} options - Counter options
 */
export function createReactiveCounter(element, options = {}) {
  const {
    duration = 300,
    formatter = (n) => n.toLocaleString(),
  } = options;

  let currentValue = parseInt(element.textContent, 10) || 0;
  let animationFrame = null;

  return {
    update(newValue) {
      if (animationFrame) {
        cancelAnimationFrame(animationFrame);
      }

      const startValue = currentValue;
      const startTime = performance.now();
      const range = newValue - startValue;

      function animate(time) {
        const elapsed = time - startTime;
        const progress = Math.min(elapsed / duration, 1);
        const easedProgress = easings.easeOutExpo(progress);

        currentValue = Math.round(startValue + (range * easedProgress));
        element.textContent = formatter(currentValue);

        if (progress < 1) {
          animationFrame = requestAnimationFrame(animate);
        } else {
          currentValue = newValue;
          element.textContent = formatter(newValue);
        }
      }

      animationFrame = requestAnimationFrame(animate);
    },

    getValue() {
      return currentValue;
    },

    setValue(value) {
      currentValue = value;
      element.textContent = formatter(value);
    }
  };
}

/**
 * Format large numbers with abbreviated suffixes
 * @param {number} num - Number to format
 * @returns {string} Formatted string (e.g., "1.2M", "3.5K")
 */
export function formatCompact(num) {
  if (num >= 1e9) return (num / 1e9).toFixed(1) + 'B';
  if (num >= 1e6) return (num / 1e6).toFixed(1) + 'M';
  if (num >= 1e3) return (num / 1e3).toFixed(1) + 'K';
  return num.toLocaleString();
}

/**
 * Format time duration
 * @param {number} seconds - Duration in seconds
 * @returns {string} Formatted duration
 */
export function formatDuration(seconds) {
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${mins}m`;
}

// Auto-initialize on DOM ready
if (typeof document !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initCountUpElements);
  } else {
    initCountUpElements();
  }
}
