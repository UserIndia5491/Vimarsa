# Vimarśa

### Understand a codebase before you start changing it.

Someone sends you a GitHub repository.

It has 200 files.
Three folders called `core`.
A README that explains almost nothing.

So you start clicking around.

Vimarśa is built for that exact moment.

Paste a public GitHub repository or open a local project, and Vimarśa gives you a clear picture of what is inside — the structure, important files, dependencies, symbols, entry points, unfinished work, and how different parts of the project relate.

Then, when you want a human explanation, you can ask an AI model to explain that picture in plain language.

**See the codebase first. Explain it second.**

---

## What Vimarśa gives you

### Overview

Start with the big picture.

See the project's languages, size, structure, entry points, configuration, tests, detected technologies, and architectural signals without opening dozens of files.

### Files

Every file is given context instead of just a filename.

You can see its likely role, important symbols, imports, exports, metrics, and review flags.

### Dependencies

See what the project depends on and how its own modules connect.

This makes it easier to spot important parts of the codebase and understand what sits around them.

### TODOs

Find `TODO`, `FIXME`, `HACK`, and other work markers across the project.

Useful when you want to know what has been left unfinished before you start making changes.

### References

Ever wondered:

> “Where is this function actually used?”

Vimarśa can show detected references and where a symbol is defined, with confidence information so you know when something is a strong match versus a heuristic one.

### Search

Search the analysis by file, symbol, import, or export and jump straight to the part you care about.

---

## AI explanations, without handing over your whole codebase

Vimarśa's AI feature is optional.

You can connect your own API key and use:

**Groq · OpenAI · OpenRouter · Google Gemini · Anthropic**

When you request an explanation, Vimarśa sends a compact summary of the analysis — not your entire source tree — along with relevant repository documentation.

The goal is simple:

**Let the scanner collect the facts. Let the model explain them.**

That also means the AI can say when something cannot be determined instead of pretending it knows how the application behaves at runtime.

---

## Local first

Repository scanning happens on your machine.

For GitHub repositories, Vimarśa fetches the public repository you explicitly provide and then performs the analysis locally.

For local folders, the project stays on your computer during analysis.

Your API key is stored locally by the application and is only used when you request an AI explanation.

No account is required just to analyze a repository.

---

## What Vimarśa is not

Vimarśa is not an AI coding agent.

It does not try to:

* rewrite your project
* automatically fix bugs
* modify your files
* pretend static analysis proves runtime behavior

It is a map, not an autopilot.

You still make the engineering decisions.

---

## A simple workflow

```text
Open a repository
      ↓
Understand the overview
      ↓
Explore important files
      ↓
Trace dependencies and references
      ↓
Check TODOs and configuration
      ↓
Ask AI for an explanation when you need one
```

The point is not to replace reading code.

It is to make the first hour of reading a lot less confusing.

---

## Built to be useful on unfamiliar projects

Whether you're:

* exploring an open-source project
* joining an existing codebase
* studying how a real application is structured
* reviewing a project
* trying to understand your own old code

Vimarśa gives you somewhere to start.

Instead of asking:

> “Which file do I even open first?”

you can start with:

> “Okay. I can see how this thing is put together.”

---

## Download

Vimarśa is distributed as a desktop application for Windows and Linux.

Get the latest release from the **Releases** section of this repository.

---

## A note about accuracy

Vimarśa uses static analysis, so some results are necessarily estimates.

That is why the application distinguishes between detected facts and heuristic conclusions where it matters.

Think of the output as a **navigation map**, not a formal proof of how the program behaves at runtime.

---

## The idea behind Vimarśa

Most code-reading tools give you either:

**a pile of files**

or

**an AI that gives you an answer.**

Vimarśa sits in between.

It builds the picture first.

Then you decide what you want to understand.

---

### Vimarśa

**See the codebase clearly. Then decide what to do with it.**
