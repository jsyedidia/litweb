(() => {
  "use strict";

  const root = document.documentElement;
  const formulas = document.querySelectorAll(".litweb-math");
  let errors = 0;

  function updateAggregateReady() {
    const mathReady =
      !root.hasAttribute("data-litweb-math-ready") ||
      root.getAttribute("data-litweb-math-ready") === "true";
    const highlightReady =
      !root.hasAttribute("data-litweb-highlight-ready") ||
      root.getAttribute("data-litweb-highlight-ready") === "true";
    if (
      mathReady &&
      highlightReady &&
      root.getAttribute("data-litweb-ready") !== "true"
    ) {
      root.setAttribute("data-litweb-ready", "true");
      root.dispatchEvent(new CustomEvent("litweb-ready"));
    }
  }

  function reportError(element, error) {
    errors += 1;
    const message =
      error instanceof Error ? error.message : String(error);
    element.classList.add("litweb-math-error");
    element.setAttribute("aria-invalid", "true");
    const explanation = document.createElement("span");
    explanation.className = "litweb-math-error-message";
    explanation.setAttribute("role", "note");
    explanation.textContent = `Math error: ${message}`;
    element.append(" ", explanation);
    console.error("Litweb could not render an equation:", error, element);
  }

  function renderFormulas() {
    for (const element of formulas) {
      const source = element.textContent || "";
      if (typeof katex === "undefined") {
        reportError(element, new Error("KaTeX did not load"));
        continue;
      }
      try {
        katex.render(source, element, {
          displayMode: element.classList.contains("litweb-math-display"),
          output: "htmlAndMathml",
          throwOnError: true,
          trust: false,
        });
        element.classList.add("litweb-math-rendered");
      } catch (error) {
        element.textContent = source;
        reportError(element, error);
      }
    }

    const fontsReady = document.fonts
      ? document.fonts.ready
      : Promise.resolve();
    Promise.resolve(fontsReady).finally(() => {
      root.setAttribute("data-litweb-math-ready", "true");
      root.dispatchEvent(
        new CustomEvent("litweb-math-ready", {
          detail: { errors },
        }),
      );
      updateAggregateReady();
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", renderFormulas, {
      once: true,
    });
  } else {
    renderFormulas();
  }
})();
