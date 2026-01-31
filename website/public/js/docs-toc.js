/**
 * Docs Table of Contents - Scroll Spy
 * Automatically generates TOC from h2/h3 headings and tracks scroll position
 */

(function() {
  'use strict';

  // Wait for DOM to be ready
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  function init() {
    const tocList = document.getElementById('docs-toc-list');
    const docsContent = document.querySelector('.docs-content');

    // Exit if TOC or content not found
    if (!tocList || !docsContent) {
      return;
    }

    // Find all h2 and h3 headings with IDs
    const headings = docsContent.querySelectorAll('h2[id], h3[id]');

    if (headings.length === 0) {
      // No headings found, hide TOC
      const tocSidebar = document.querySelector('.docs-toc-sidebar');
      if (tocSidebar) {
        tocSidebar.style.display = 'none';
      }
      return;
    }

    // Generate TOC links
    headings.forEach((heading) => {
      const id = heading.id;
      const text = heading.textContent;
      const level = heading.tagName.toLowerCase();

      const li = document.createElement('li');
      li.className = 'docs-toc__item';

      // Add nested class for h3
      if (level === 'h3') {
        li.classList.add('docs-toc__item--nested');
      }

      const a = document.createElement('a');
      a.className = 'docs-toc__link';
      a.href = `#${id}`;
      a.textContent = text;

      li.appendChild(a);
      tocList.appendChild(li);
    });

    // Set up Intersection Observer for scroll tracking
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            const id = entry.target.id;

            // Remove active class from all links
            document.querySelectorAll('.docs-toc__link').forEach((link) => {
              link.classList.remove('is-active');
            });

            // Add active class to matching link
            const activeLink = document.querySelector(`.docs-toc__link[href="#${id}"]`);
            if (activeLink) {
              activeLink.classList.add('is-active');
            }
          }
        });
      },
      {
        rootMargin: '-80px 0px -80% 0px',
        threshold: 0
      }
    );

    // Observe all headings
    headings.forEach((heading) => observer.observe(heading));

    // Handle click events on TOC links (smooth scroll)
    tocList.addEventListener('click', (e) => {
      if (e.target.classList.contains('docs-toc__link')) {
        e.preventDefault();
        const targetId = e.target.getAttribute('href').slice(1);
        const targetElement = document.getElementById(targetId);

        if (targetElement) {
          targetElement.scrollIntoView({
            behavior: 'smooth',
            block: 'start'
          });

          // Update URL without scrolling
          history.pushState(null, null, `#${targetId}`);
        }
      }
    });

    // Set initial active state based on scroll position
    const firstHeading = headings[0];
    if (firstHeading) {
      const firstLink = document.querySelector(`.docs-toc__link[href="#${firstHeading.id}"]`);
      if (firstLink) {
        firstLink.classList.add('is-active');
      }
    }
  }
})();
