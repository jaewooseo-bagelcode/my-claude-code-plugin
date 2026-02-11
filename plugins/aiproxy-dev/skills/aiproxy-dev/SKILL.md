---
name: aiproxy-dev
description: AIProxy API development guide. Ensures accurate endpoints, auth, models, and SDK patterns when implementing code that calls AI APIs (OpenAI, Anthropic, Google, ElevenLabs, Moonshot) through AIProxy. Auto-reference when writing AI API integration code.
---

# AIProxy Development Guide

Reference guide for implementing code that calls AI APIs through the BagelCode AIProxy.

## Core Principle

**Always verify with `codeb docs` and `codeb models` before implementing.** Never guess model IDs, endpoints, or parameters.

## Information Gathering Commands

### API Reference (on-demand)

```bash
# Full API reference (markdown)
codeb docs

# Single command only (lightweight — prefer this)
codeb docs --command chat
codeb docs --command explore
codeb docs --command image
codeb docs --command tts
codeb docs --command sfx
codeb docs --command embed
codeb docs --command braintrust

# JSON format (for parsing)
codeb docs --command chat --format json
```

### Available Models

```bash
# All models (model ID, provider, context window, pricing)
codeb models --json

# Filter by provider
codeb models --provider openai --json
codeb models --provider anthropic --json
codeb models --provider google --json
codeb models --provider moonshot --json
codeb models --provider elevenlabs --json
```

### Voices (for TTS/STS)

```bash
codeb voices --json
```

### Auth Status

```bash
codeb whoami
```

## API Passthrough Architecture

AIProxy passes requests through to each provider API **as-is**. Request/response formats are identical to the original APIs. Only authentication is replaced with the codeb token.

### Base URL

```
https://aiproxy-api.backoffice.bagelgames.com
```

### Authentication

```bash
# Token location
cat ~/.codeb/credentials.json
# → {"token": "aiproxy_xxx", "email": "..."}
```

All requests require:
```
Authorization: Bearer aiproxy_xxx
```

### Endpoint Summary

| Provider | Endpoint | Original API |
|----------|----------|-------------|
| **OpenAI** | `POST /openai/v1/responses` | Responses API |
| | `POST /openai/v1/chat/completions` | Chat Completions API |
| | `POST /openai/v1/embeddings` | Embeddings API |
| | `POST /openai/v1/images/generations` | Image Generation API |
| **Anthropic** | `POST /anthropic/v1/messages` | Messages API |
| **Google** | `POST /google/v1beta/models/{model}:generateContent` | Gemini API |
| | `POST /google/v1beta/models/{model}:streamGenerateContent` | Gemini Streaming |
| | `POST /google/v1beta/models/{model}:embedContent` | Gemini Embeddings |
| **Google Vertex** | `POST /google-vertex/v1beta/models/{model}:generateContent` | Vertex AI |
| **Moonshot** | `POST /moonshot/v1/chat/completions` | Kimi API (OpenAI-compatible) |
| **ElevenLabs** | `POST /elevenlabs/v1/text-to-speech/{voiceId}` | TTS |
| | `POST /elevenlabs/v1/sound-generation` | SFX |
| | `POST /elevenlabs/v1/speech-to-speech/{voiceId}` | STS |
| | `POST /elevenlabs/v1/audio-isolation` | Noise Removal |

### Important Notes

- `/conversations` endpoint is **NOT supported** — use `previous_response_id` chaining for OpenAI
- Google/Vertex uses **Bearer token** auth (not API key) unlike the original API
- Streaming: returns each provider's native SSE format as-is
- Error codes: 401 (auth), 403 (budget exceeded), 429 (rate limit), 504 (timeout)

## codeb CLI for Quick Prototyping

Test API calls with codeb CLI before writing code:

```bash
# Chat
codeb chat openai "Hello"
codeb chat anthropic/claude-opus-4-6 "Explain this"

# Code exploration (agentic loop)
codeb explore openai "Analyze project structure"

# Image generation
codeb image "A diagram" -o diagram.png

# TTS
codeb tts "Hello" --voice <voice_id> -o hello.mp3

# Embeddings
codeb embed "Hello world" --json

# Sound effects
codeb sfx "Button click" -o click.mp3
```

## Implementation Checklist

Before writing AI API call code, verify:

1. **Model ID**: Run `codeb models --provider <name> --json` for exact IDs
2. **Endpoint**: Run `codeb docs --command <cmd>` for exact paths
3. **Auth header**: `Authorization: Bearer` + token from `~/.codeb/credentials.json`
4. **Request format**: Each provider uses its native API format (OpenAI ≠ Anthropic ≠ Google)
5. **Streaming**: SSE parsing differs by provider — check docs before implementing
6. **Error handling**: Handle 401, 403, 429, 504 responses
