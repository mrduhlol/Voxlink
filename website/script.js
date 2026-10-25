/**
 * Voxlink Website - Minimal JavaScript
 * Only handles smooth scroll for anchor links and basic enhancements
 */

(function() {
  'use strict';

  // Smooth scroll for anchor links (progressive enhancement)
  document.querySelectorAll('a[href^="#"]').forEach(function(anchor) {
    anchor.addEventListener('click', function(e) {
      var targetId = this.getAttribute('href');
      if (targetId === '#') return;

      var target = document.querySelector(targetId);
      if (target) {
        e.preventDefault();
        var headerOffset = 72; // Account for sticky header
        var elementPosition = target.getBoundingClientRect().top;
        var offsetPosition = elementPosition + window.pageYOffset - headerOffset;

        window.scrollTo({
          top: offsetPosition,
          behavior: 'smooth'
        });

        // Update focus for accessibility
        target.setAttribute('tabindex', '-1');
        target.focus({ preventScroll: true });
        target.removeAttribute('tabindex');
      }
    });
  });

  // Add current year to any copyright elements
  var yearElements = document.querySelectorAll('[data-year]');
  var currentYear = new Date().getFullYear();
  yearElements.forEach(function(el) {
    el.textContent = currentYear;
  });

  // Detect if we're on GitHub Pages and add a subtle indicator
  if (window.location.hostname.includes('github.io')) {
    document.body.classList.add('github-pages');
  }

  // Simple copy-to-clipboard for code blocks (if needed)
  document.querySelectorAll('pre code').forEach(function(codeBlock) {
    codeBlock.addEventListener('click', function() {
      var text = this.textContent;
      navigator.clipboard.writeText(text).then(function() {
        var originalTitle = this.title;
        this.title = 'Copied!';
        setTimeout(function() {
          this.title = originalTitle;
        }.bind(this), 1500);
      }.bind(this));
    });
  });

})();