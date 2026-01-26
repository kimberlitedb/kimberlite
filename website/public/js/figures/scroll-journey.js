/**
 * Scroll Journey Module
 * JavaScript fallback for scroll-driven animations
 * Used when CSS scroll-timeline is not supported
 */

/**
 * Check if CSS scroll-driven animations are supported
 * @returns {boolean}
 */
export function supportsScrollTimeline() {
  return CSS.supports('animation-timeline', 'scroll()');
}

/**
 * Calculate scroll progress within an element
 * @param {HTMLElement} element - The scroll journey container
 * @returns {number} Progress from 0 to 1
 */
function getScrollProgress(element) {
  const rect = element.getBoundingClientRect();
  const viewportHeight = window.innerHeight;

  // Element top relative to viewport
  const elementTop = rect.top;
  // How far into the scroll we are
  const scrollDistance = -elementTop;
  // Total scrollable distance (element height minus one viewport)
  const totalDistance = rect.height - viewportHeight;

  if (totalDistance <= 0) return 0;

  return Math.max(0, Math.min(1, scrollDistance / totalDistance));
}

/**
 * Get the current stage based on scroll progress
 * @param {number} progress - Scroll progress (0-1)
 * @param {number[]} thresholds - Array of threshold values for each stage
 * @returns {number} Current stage index
 */
function getCurrentStage(progress, thresholds) {
  for (let i = thresholds.length - 1; i >= 0; i--) {
    if (progress >= thresholds[i]) {
      return i;
    }
  }
  return -1;
}

/**
 * Initialize scroll-driven journey animation
 * @param {HTMLElement} container - The scroll journey container
 * @param {Object} options - Configuration options
 */
export function initScrollJourney(container, options = {}) {
  // Skip if native CSS scroll-timeline is supported
  if (supportsScrollTimeline()) {
    console.log('Using native CSS scroll-driven animations');
    return null;
  }

  const {
    stageThresholds = [0, 0.15, 0.30, 0.45, 0.60, 0.70, 0.80, 0.90, 0.95],
    onStageChange = null,
    onProgress = null,
  } = options;

  let currentStage = -1;
  let rafId = null;

  function update() {
    const progress = getScrollProgress(container);

    // Update progress bar if exists
    const progressBar = container.querySelector('.scroll-journey__progress-bar');
    if (progressBar) {
      progressBar.style.height = `${progress * 100}%`;
    }

    // Update stage markers
    const markers = container.querySelectorAll('.scroll-journey__progress-marker');
    markers.forEach((marker, index) => {
      marker.setAttribute('data-active', progress >= stageThresholds[index] ? 'true' : 'false');
    });

    // Determine current stage
    const newStage = getCurrentStage(progress, stageThresholds);

    if (newStage !== currentStage) {
      currentStage = newStage;
      container.setAttribute('data-stage', currentStage.toString());

      if (onStageChange) {
        onStageChange(currentStage, progress);
      }
    }

    // Custom progress callback
    if (onProgress) {
      onProgress(progress, currentStage);
    }

    // Set CSS custom properties for fine-grained control
    container.style.setProperty('--scroll-progress', progress.toString());

    // Calculate per-stage progress
    stageThresholds.forEach((threshold, index) => {
      const nextThreshold = stageThresholds[index + 1] || 1;
      const range = nextThreshold - threshold;
      const stageProgress = Math.max(0, Math.min(1, (progress - threshold) / range));

      container.style.setProperty(`--stage-${index}-opacity`, stageProgress >= 0 ? '1' : '0');
      container.style.setProperty(`--stage-${index}-progress`, stageProgress.toString());
    });

    rafId = requestAnimationFrame(update);
  }

  // Start the animation loop
  update();

  // Return cleanup function
  return () => {
    if (rafId) {
      cancelAnimationFrame(rafId);
    }
  };
}

/**
 * Initialize all scroll journey elements on the page
 */
export function initAllScrollJourneys() {
  const journeys = document.querySelectorAll('.scroll-journey');

  const cleanupFunctions = [];

  journeys.forEach(journey => {
    const cleanup = initScrollJourney(journey);
    if (cleanup) {
      cleanupFunctions.push(cleanup);
    }
  });

  return () => {
    cleanupFunctions.forEach(fn => fn());
  };
}

/**
 * Animate message paths in the consensus diagram
 * @param {SVGElement} svg - The SVG element containing message paths
 * @param {Object} options - Animation options
 */
export function animateMessagePaths(svg, options = {}) {
  const {
    duration = 500,
    stagger = 100,
    onComplete = null,
  } = options;

  const paths = svg.querySelectorAll('.scroll-journey__message-path');

  paths.forEach((path, index) => {
    const length = path.getTotalLength();

    // Set up initial state
    path.style.strokeDasharray = length;
    path.style.strokeDashoffset = length;

    // Animate after stagger delay
    setTimeout(() => {
      path.style.transition = `stroke-dashoffset ${duration}ms ease-out`;
      path.style.strokeDashoffset = '0';
    }, index * stagger);
  });

  // Call complete callback after all animations
  if (onComplete) {
    const totalDuration = (paths.length - 1) * stagger + duration;
    setTimeout(onComplete, totalDuration);
  }
}

/**
 * Type out text character by character (for hash computation animation)
 * @param {HTMLElement} element - Element to type into
 * @param {string} text - Text to type
 * @param {Object} options - Animation options
 */
export function typeText(element, text, options = {}) {
  const {
    speed = 30, // ms per character
    onComplete = null,
  } = options;

  let index = 0;
  element.textContent = '';

  function type() {
    if (index < text.length) {
      element.textContent += text[index];
      index++;
      setTimeout(type, speed);
    } else if (onComplete) {
      onComplete();
    }
  }

  type();
}

// Auto-initialize on DOM ready
if (typeof document !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initAllScrollJourneys);
  } else {
    // DOM already loaded, initialize immediately
    initAllScrollJourneys();
  }
}
