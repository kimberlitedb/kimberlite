/**
 * Lab Clock Component
 * SVG clock with draggable hands for time travel and simulation figures
 */

/**
 * Generate SVG tick marks for the clock face
 * @param {number} count - Number of tick marks (default 60 for minute marks)
 * @param {number} majorEvery - Every Nth tick is a major tick (default 5)
 * @returns {string} SVG string for tick marks
 */
export function generateTickMarks(count = 60, majorEvery = 5) {
  const ticks = [];
  const radius = 50; // SVG uses 0-100 viewBox
  const center = 50;

  for (let i = 0; i < count; i++) {
    const angle = (i / count) * 2 * Math.PI - Math.PI / 2; // Start at 12 o'clock
    const isMajor = i % majorEvery === 0;

    const innerRadius = isMajor ? radius - 10 : radius - 6;
    const outerRadius = radius - 3;

    const x1 = center + innerRadius * Math.cos(angle);
    const y1 = center + innerRadius * Math.sin(angle);
    const x2 = center + outerRadius * Math.cos(angle);
    const y2 = center + outerRadius * Math.sin(angle);

    const className = isMajor ? 'lab-clock__tick--major' : 'lab-clock__tick--minor';
    ticks.push(`<line x1="${x1.toFixed(2)}" y1="${y1.toFixed(2)}" x2="${x2.toFixed(2)}" y2="${y2.toFixed(2)}" class="${className}"/>`);
  }

  return ticks.join('\n');
}

/**
 * Generate crosshairs SVG
 * @returns {string} SVG string for crosshairs
 */
export function generateCrosshairs() {
  return `
    <line x1="50" y1="15" x2="50" y2="85" />
    <line x1="15" y1="50" x2="85" y2="50" />
  `;
}

/**
 * Convert position value (0-100) to rotation angle in degrees
 * @param {number} position - Position value (0-100)
 * @param {number} maxPosition - Maximum position value
 * @returns {number} Rotation angle in degrees
 */
export function positionToAngle(position, maxPosition = 100) {
  return (position / maxPosition) * 360;
}

/**
 * Convert angle to position value
 * @param {number} angle - Angle in degrees (0-360)
 * @param {number} maxPosition - Maximum position value
 * @returns {number} Position value (0-maxPosition)
 */
export function angleToPosition(angle, maxPosition = 100) {
  // Normalize angle to 0-360
  let normalizedAngle = angle % 360;
  if (normalizedAngle < 0) normalizedAngle += 360;
  return Math.round((normalizedAngle / 360) * maxPosition);
}

/**
 * Calculate angle from center point to a given point
 * @param {number} cx - Center X
 * @param {number} cy - Center Y
 * @param {number} px - Point X
 * @param {number} py - Point Y
 * @returns {number} Angle in degrees (0 at top, clockwise)
 */
export function getAngleFromCenter(cx, cy, px, py) {
  const dx = px - cx;
  const dy = py - cy;
  let angle = Math.atan2(dy, dx) * (180 / Math.PI);
  // Convert from standard angle (0 = right, counterclockwise) to clock angle (0 = top, clockwise)
  angle = angle + 90;
  if (angle < 0) angle += 360;
  return angle;
}

/**
 * Initialize draggable clock hand
 * @param {HTMLElement} clockElement - The clock container element
 * @param {Function} onPositionChange - Callback when position changes
 * @param {Object} options - Configuration options
 */
export function initDraggableClock(clockElement, onPositionChange, options = {}) {
  const {
    maxPosition = 100,
    minPosition = 0,
    snapToIncrements = 1,
    handSelector = '.lab-clock__hand--main',
    interactionSelector = '.lab-clock__interaction',
  } = options;

  const interactionArea = clockElement.querySelector(interactionSelector);
  if (!interactionArea) return;

  let isDragging = false;
  let currentPosition = 0;

  function updatePosition(clientX, clientY) {
    const rect = clockElement.getBoundingClientRect();
    const centerX = rect.left + rect.width / 2;
    const centerY = rect.top + rect.height / 2;

    const angle = getAngleFromCenter(centerX, centerY, clientX, clientY);
    let position = angleToPosition(angle, maxPosition);

    // Snap to increments
    if (snapToIncrements > 1) {
      position = Math.round(position / snapToIncrements) * snapToIncrements;
    }

    // Clamp to range
    position = Math.max(minPosition, Math.min(maxPosition, position));

    if (position !== currentPosition) {
      currentPosition = position;
      onPositionChange(position);
    }
  }

  function onPointerDown(e) {
    isDragging = true;
    clockElement.setAttribute('data-dragging', 'true');
    interactionArea.setPointerCapture(e.pointerId);
    updatePosition(e.clientX, e.clientY);
  }

  function onPointerMove(e) {
    if (!isDragging) return;
    updatePosition(e.clientX, e.clientY);
  }

  function onPointerUp(e) {
    if (!isDragging) return;
    isDragging = false;
    clockElement.removeAttribute('data-dragging');
    interactionArea.releasePointerCapture(e.pointerId);
  }

  // Prevent text selection during drag
  function onDragStart(e) {
    e.preventDefault();
  }

  interactionArea.addEventListener('pointerdown', onPointerDown);
  interactionArea.addEventListener('pointermove', onPointerMove);
  interactionArea.addEventListener('pointerup', onPointerUp);
  interactionArea.addEventListener('pointercancel', onPointerUp);
  interactionArea.addEventListener('dragstart', onDragStart);

  // Return cleanup function
  return () => {
    interactionArea.removeEventListener('pointerdown', onPointerDown);
    interactionArea.removeEventListener('pointermove', onPointerMove);
    interactionArea.removeEventListener('pointerup', onPointerUp);
    interactionArea.removeEventListener('pointercancel', onPointerUp);
    interactionArea.removeEventListener('dragstart', onDragStart);
  };
}

/**
 * Update clock hand rotation
 * @param {HTMLElement} handElement - The hand element
 * @param {number} position - Position value
 * @param {number} maxPosition - Maximum position value
 */
export function updateHandRotation(handElement, position, maxPosition = 100) {
  const angle = positionToAngle(position, maxPosition);
  handElement.style.transform = `rotate(${angle}deg)`;
}

/**
 * Create a simulation clock that spins based on speed multiplier
 * @param {HTMLElement} clockElement - The clock container
 * @param {Object} options - Simulation options
 */
export function createSimulationClock(clockElement, options = {}) {
  const {
    handSelector = '.lab-clock__hand--main',
    secondaryHandSelector = '.lab-clock__hand--secondary',
    onTick = null,
  } = options;

  const mainHand = clockElement.querySelector(handSelector);
  const secondaryHand = clockElement.querySelector(secondaryHandSelector);

  let animationFrame = null;
  let startTime = null;
  let speed = 1;
  let baseAngle = 0;
  let isRunning = false;
  let tickCount = 0;

  function animate(timestamp) {
    if (!startTime) startTime = timestamp;
    const elapsed = timestamp - startTime;

    // Calculate rotation based on speed
    // 1x = 1 rotation per minute (6 degrees per second)
    // 100x = 100 rotations per minute
    // 1000x = 1000 rotations per minute
    const degreesPerMs = (6 * speed) / 1000;
    const currentAngle = baseAngle + (elapsed * degreesPerMs);

    if (mainHand) {
      mainHand.style.transform = `rotate(${currentAngle}deg)`;
    }

    if (secondaryHand) {
      // Secondary hand moves faster (simulated seconds)
      secondaryHand.style.transform = `rotate(${currentAngle * 12}deg)`;
    }

    // Tick callback every "second" of simulated time
    const newTickCount = Math.floor((elapsed * speed) / 1000);
    if (newTickCount > tickCount && onTick) {
      tickCount = newTickCount;
      onTick(tickCount, elapsed);
    }

    if (isRunning) {
      animationFrame = requestAnimationFrame(animate);
    }
  }

  return {
    start(initialSpeed = 1) {
      speed = initialSpeed;
      isRunning = true;
      startTime = null;
      tickCount = 0;
      animationFrame = requestAnimationFrame(animate);
    },

    stop() {
      isRunning = false;
      if (animationFrame) {
        cancelAnimationFrame(animationFrame);
        animationFrame = null;
      }
      // Save current angle for resume
      if (mainHand) {
        const transform = mainHand.style.transform;
        const match = transform.match(/rotate\(([\d.]+)deg\)/);
        if (match) {
          baseAngle = parseFloat(match[1]) % 360;
        }
      }
    },

    setSpeed(newSpeed) {
      if (isRunning) {
        // Save current state and restart with new speed
        this.stop();
        speed = newSpeed;
        this.start(speed);
      } else {
        speed = newSpeed;
      }
    },

    reset() {
      this.stop();
      baseAngle = 0;
      tickCount = 0;
      if (mainHand) mainHand.style.transform = 'rotate(0deg)';
      if (secondaryHand) secondaryHand.style.transform = 'rotate(0deg)';
    },

    isRunning() {
      return isRunning;
    },

    getSpeed() {
      return speed;
    }
  };
}
