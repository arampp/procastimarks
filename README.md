# 🔖 Procastimarks

> Capture now. Read later. Procrastinate forever.

Ever feel like you've already read the entire internet — yet somehow there's
nothing left to procrastinate with? Procastimarks fixes that. Save anything
interesting in one click, and you'll always have a curated personal backlog of
rabbit holes waiting for you the next time focus feels optional.

Under the hood it is a single-user, self-hosted bookmark manager: one click
saves a URL with its title and description; a full-text search finds it again
weeks later by topic — not by trying to remember the exact title or address.

---

## 🎯 Project Goals

| # | Goal |
|---|------|
| **G-1** | Capture a bookmark in a single browser action with no context-switch. |
| **G-2** | Retrieve any saved bookmark by topic within a few seconds. |
| **G-3** | Access the collection from any device via a web browser. |
| **G-4** | Prove that a full-stack web application can be built entirely in the Rust programming language. |
| **G-5** | Evaluate lightweight, [Semantic Anchors](https://llm-coding.github.io/Semantic-Anchors/#/workflow)-driven AI-assisted development as an alternative to heavyweight spec-driven agent frameworks. |

---

## 🚀 Features

- **Bookmarklet capture** — a single browser toolbar click opens a pre-filled
  form with the page URL, title, and meta description fetched server-side.
- **Full-text search** — unified search-as-you-type across title, description,
  comment, and tags, powered by SQLite FTS5.
- **Tag filtering & autocomplete** — organise bookmarks with free-form tags;
  autocomplete keeps them consistent.
- **Delete with confirmation** — remove bookmarks safely without accidental loss.
- **API-key authentication** — lightweight single-user protection; no accounts,
  no user management.
- **Vintage UI** — early-2000s aesthetic: system fonts, muted colours, minimal
  decoration.

---

## 🧱 Technology Stack

| Layer | Choice |
|-------|--------|
| Language | Rust (backend **and** frontend) |
| Backend | Axum |
| Frontend | Leptos (server functions, no separate REST API) |
| Database | SQLite + FTS5 |
| Deployment | Docker / Docker Compose |

---

## 🔬 Research Goal: Semantic Anchors

A secondary aim of this project is to explore whether the
[Semantic Anchors Catalogue](https://llm-coding.github.io/Semantic-Anchors/#/workflow)
— a curated set of well-defined terms, methodologies, and frameworks for
precise communication with LLMs — is sufficient to guide AI-assisted software
development without resorting to heavy, opinionated agent frameworks.

The hypothesis is that a compact, shared vocabulary of semantic anchors gives an
AI coding assistant enough grounding to produce correct, idiomatic code, while
keeping the developer in full control of the process and the toolchain.

---

## 💡 Inspiration

- **del.icio.us** — the original tag-based bookmark experience (social features
  excluded).
- **raindrop.io** — a modern visual bookmark manager (visual richness traded for
  a vintage aesthetic).

---

## ⚠️ Non-Goals

The following are explicitly out of scope:

- Social or collaborative bookmark sharing
- A browser extension (the bookmarklet is browser-agnostic)
- Native mobile applications
- A read-it-later / article-reader feature
- A knowledge graph or second-brain system
- Multi-user accounts or role-based access control

---

*Procastimarks — your curated backlog of the internet, patiently waiting for your next procrastination session.*
