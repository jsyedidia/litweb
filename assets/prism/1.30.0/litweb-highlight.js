(function () {
    "use strict";

    var root = document.documentElement;
    var aliases = {
        "c++": "cpp",
        "c#": "csharp",
        "f#": "fsharp",
        "objective-c": "objectivec",
        "objective-c++": "cpp",
        "shell": "bash",
        "html": "markup",
        "xml": "markup",
        "tex": "latex"
    };

    function updateAggregateReady() {
        var mathReady =
            !root.hasAttribute("data-litweb-math-ready") ||
            root.getAttribute("data-litweb-math-ready") === "true";
        var highlightReady =
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

    function finish() {
        if (root.getAttribute("data-litweb-highlight-ready") !== "true") {
            root.setAttribute("data-litweb-highlight-ready", "true");
            root.dispatchEvent(new CustomEvent("litweb-highlight-ready"));
        }
        updateAggregateReady();
    }

    function languageFor(element) {
        var candidates = (element.getAttribute("data-litweb-language") || "")
            .split(/\s+/)
            .filter(Boolean);
        for (var index = 0; index < candidates.length; index += 1) {
            var candidate = candidates[index].toLowerCase();
            candidate = aliases[candidate] || candidate;
            if (Prism.languages[candidate]) {
                return candidate;
            }
        }
        return null;
    }

    function highlightTextNode(node, grammar, language) {
        if (!node.data) {
            return;
        }
        var highlighted = document.createElement("span");
        highlighted.innerHTML = Prism.highlight(node.data, grammar, language);
        var parent = node.parentNode;
        while (highlighted.firstChild) {
            parent.insertBefore(highlighted.firstChild, node);
        }
        parent.removeChild(node);
    }

    function highlightTextAroundMarkup(element, grammar, language) {
        Array.prototype.slice
            .call(element.childNodes)
            .forEach(function (node) {
                if (node.nodeType === Node.TEXT_NODE) {
                    highlightTextNode(node, grammar, language);
                }
            });
    }

    function highlightAll() {
        if (typeof Prism === "undefined") {
            console.error("Litweb syntax highlighting could not load Prism.");
            finish();
            return;
        }

        document
            .querySelectorAll("code[data-litweb-highlight]")
            .forEach(function (element) {
                if (element.getAttribute("data-litweb-highlight") === "true") {
                    return;
                }
                var language = languageFor(element);
                if (!language) {
                    element.setAttribute("data-litweb-highlight", "unsupported");
                    return;
                }
                var original = element.innerHTML;
                try {
                    Prism.util.setLanguage(element, language);
                    if (element.parentElement && element.parentElement.tagName === "PRE") {
                        Prism.util.setLanguage(element.parentElement, language);
                    }
                    highlightTextAroundMarkup(
                        element,
                        Prism.languages[language],
                        language
                    );
                    element.setAttribute("data-litweb-highlight", "true");
                } catch (error) {
                    element.innerHTML = original;
                    element.setAttribute("data-litweb-highlight", "error");
                    console.error(
                        "Litweb could not syntax-highlight a " + language + " fragment.",
                        error
                    );
                }
            });

        var fontsReady =
            document.fonts && document.fonts.ready
                ? document.fonts.ready
                : Promise.resolve();
        Promise.resolve(fontsReady).then(finish, finish);
    }

    window.litwebHighlight = highlightAll;
    highlightAll();
}());
