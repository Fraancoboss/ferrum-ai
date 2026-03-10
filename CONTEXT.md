Quiero haver un proyecto en rust que:

Sea un chatbot muy simple (en el front)

Y en ve de usar una api key lo que haga el programa por detras sea:

Cada: new chat crea una instancia de una terminal interna (PTY creo) y use comandos:

codex exec “prompt escrito en el front”

claude exec “prompt escrito en el front”

Y que “scrappe” el output de la terminal, luego persista todo en una db postgres en docker para usar lo generado como un RAG
Pensó durante 37s

Sí, y además te diría que no necesitas montar una PTY para el camino feliz.

Tanto Codex como Claude Code ya tienen modo no interactivo para scripts:

Codex soporta codex exec "..." y puede sacar eventos en JSONL con --json. Además, por defecto el resultado final va a stdout y el progreso a stderr.

Claude Code soporta claude -p "..." y puede devolver text, json o stream-json con --output-format.

Así que para tu proyecto en Rust, mi recomendación es:

Arquitectura recomendada

Front muy simple

chat web mínimo

lista de conversaciones

caja de texto

selector de backend: codex o claude

Backend Rust

API HTTP

crea un chat_session

ejecuta el proveedor elegido como proceso hijo

captura stdout/stderr

persiste:

prompt del usuario

output final

logs/eventos

metadata del modelo/proveedor

opcionalmente genera embeddings y los guarda en Postgres para RAG

Postgres en Docker

tablas relacionales para chats/mensajes/runs

extensión vectorial para RAG con pgvector

Lo importante: PTY vs subprocess normal

Tu idea de “crear una terminal interna (PTY)” tiene sentido solo si quieres emular una terminal real porque alguna CLI lo exige o porque quieres reproducir una UX interactiva. En Rust, portable-pty existe precisamente para trabajar con pseudo-terminales de forma cross-platform.

Pero para lo que describes, yo haría esto:

Opción A — la buena para V1

Usar tokio::process::Command y lanzar:

codex exec --json "<prompt>"

claude -p "<prompt>" --output-format stream-json

Ventajas:

más simple

más estable

más fácil de parsear

menos problemas de buffering, señales, resize, control chars

Opción B — PTY

Usar portable-pty solo si luego quieres:

mostrar salida “tipo terminal”

manejar UIs interactivas reales

soportar CLIs que se comportan distinto sin TTY

Para tu MVP, A > B.

Ojo con una cosa clave

Dices “en vez de usar una api key”. Eso no elimina la autenticación.

Codex CLI puede autenticarse con ChatGPT login o con API key; en la CLI el login con ChatGPT es el camino por defecto cuando no hay sesión válida.

Claude Code requiere cuenta/login; la quickstart oficial pide iniciar sesión y tener una suscripción o cuenta compatible.

O sea: tu app no usaría una API key “desde el front”, pero por detrás sí depende de que esas CLIs estén instaladas y autenticadas en la máquina/servidor. Eso es importante para el diseño y para seguridad.

Diseño de tablas en Postgres

Yo haría algo así:

chat_sessions

id UUID PK

title TEXT

provider TEXT (codex | claude)

created_at TIMESTAMPTZ

messages

id UUID PK

session_id UUID FK

role TEXT (user | assistant | system)

content TEXT

created_at TIMESTAMPTZ

runs

id UUID PK

session_id UUID FK

provider TEXT

command TEXT

status TEXT

stdout TEXT

stderr TEXT

raw_events JSONB

created_at TIMESTAMPTZ

finished_at TIMESTAMPTZ

documents

id UUID PK

session_id UUID FK NULL

source_run_id UUID FK NULL

content TEXT

metadata JSONB

embedding VECTOR(1536) o la dimensión que uses

chunks

id UUID PK

document_id UUID FK

chunk_index INT

content TEXT

embedding VECTOR(1536)

metadata JSONB

Con eso ya tienes:

histórico conversacional

auditoría de ejecuciones

base para RAG

Flujo del backend
POST /chat/new

crea chat_session

devuelve session_id

POST /chat/:id/message

guarda mensaje del usuario

resuelve proveedor (codex o claude)

ejecuta CLI

parsea salida

guarda:

mensaje assistant

run

eventos/raw JSON

chunking + embeddings

inserta chunks en Postgres

GET /chat/:id/messages

Devuelve historial

POST /rag/search

embed query

vector search en pgvector

devuelve chunks relevantes

Comandos concretos
Codex

Para automatización, la doc oficial recomienda codex exec; con --json emite un stream JSONL de eventos.

Ejemplo:

codex exec --json "Resume este proyecto"
Claude

La forma no interactiva es:

claude -p "Resume este proyecto" --output-format stream-json

o

claude -p "Resume este proyecto" --output-format json

Stack Rust que usaría

axum para la API

tokio para async

sqlx para Postgres

serde / serde_json

uuid

chrono

tokio-stream

thiserror / anyhow

opcional: portable-pty si insistes en PTY

Docker Compose mínimo

Para Postgres base, la imagen oficial usa POSTGRES_PASSWORD como variable mínima requerida.

Ejemplo:

version: "3.9"

services:
  postgres:
    image: pgvector/pgvector:pg16
    container_name: rag_postgres
    environment:
      POSTGRES_DB: chatbot
      POSTGRES_USER: chatbot
      POSTGRES_PASSWORD: chatbot
    ports:
      - "5432:5432"
    volumes:
      - pgdata:/var/lib/postgresql/data

volumes:
  pgdata:
Esquema SQL inicial
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE chat_sessions (
    id UUID PRIMARY KEY,
    title TEXT,
    provider TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE messages (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE runs (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    command TEXT NOT NULL,
    status TEXT NOT NULL,
    stdout TEXT,
    stderr TEXT,
    raw_events JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE TABLE documents (
    id UUID PRIMARY KEY,
    session_id UUID REFERENCES chat_sessions(id) ON DELETE SET NULL,
    source_run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    content TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE chunks (
    id UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index INT NOT NULL,
    content TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    embedding vector(1536)
);
Ejemplo de ejecución en Rust sin PTY
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub async fn run_codex(prompt: &str) -> anyhow::Result<(String, String)> {
    let mut child = Command::new("codex")
        .arg("exec")
        .arg("--json")
        .arg(prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut out_reader = BufReader::new(stdout).lines();
    let mut err_reader = BufReader::new(stderr).lines();

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();

    loop {
        tokio::select! {
            line = out_reader.next_line() => {
                match line? {
                    Some(l) => {
                        stdout_buf.push_str(&l);
                        stdout_buf.push('\n');
                    }
                    None => break,
                }
            }
            line = err_reader.next_line() => {
                match line? {
                    Some(l) => {
                        stderr_buf.push_str(&l);
                        stderr_buf.push('\n');
                    }
                    None => {}
                }
            }
        }
    }

    let _status = child.wait().await?;
    Ok((stdout_buf, stderr_buf))
}

Y para Claude:

pub async fn run_claude(prompt: &str) -> anyhow::Result<(String, String)> {
    let mut child = Command::new("claude")
        .arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut out_reader = BufReader::new(stdout).lines();
    let mut err_reader = BufReader::new(stderr).lines();

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();

    loop {
        tokio::select! {
            line = out_reader.next_line() => {
                match line? {
                    Some(l) => {
                        stdout_buf.push_str(&l);
                        stdout_buf.push('\n');
                    }
                    None => break,
                }
            }
            line = err_reader.next_line() => {
                match line? {
                    Some(l) => {
                        stderr_buf.push_str(&l);
                        stderr_buf.push('\n');
                    }
                    None => {}
                }
            }
        }
    }

    let _status = child.wait().await?;
    Ok((stdout_buf, stderr_buf))
}
Parsing recomendado

No “scrapees” texto libre si puedes evitarlo.

Haz esto:

Codex: parsea JSONL con serde_json::Value o structs tipadas, porque el --json ya está pensado para automatización.

Claude: usa --output-format json o stream-json y parsea línea a línea.

Eso te da:

menos fragilidad

mejor observabilidad

mejor persistencia para RAG/auditoría

Cómo convertirlo en RAG de verdad

Tu idea tiene sentido, pero separaría dos cosas:

1. Historial conversacional

Guardar prompts y respuestas tal cual.

2. Corpus RAG

No metas cada salida “sin más”.
Haz pipeline:

normalización

chunking

embeddings

indexado vectorial

recuperación por similitud

Y además guarda metadata:

provider

session_id

run_id

timestamp

topic

source_type (chat_output, summary, code, etc.)

Seguridad

Aquí hay riesgo real, porque estás exponiendo CLIs locales detrás de un front.

Mínimos:

no interpolar prompts en shell strings

usa Command::new(...).arg(...), nunca sh -c

ejecuta en contenedor aislado

limita filesystem accesible

timeouts por proceso

límites de tamaño de salida

cola de trabajos

auth de usuarios

rate limiting

Además, en Codex hay modos de permisos/sandbox; la propia documentación recomienda usar el nivel mínimo necesario y advierte sobre danger-full-access.

Mi recomendación final de implementación

Haz el proyecto en esta secuencia:

Fase 1

front simple

backend Rust

new chat

send message

ejecutar codex exec o claude -p

guardar output en Postgres

Fase 2

parseo estructurado JSON/JSONL

streaming al front por SSE/WebSocket

Fase 3

chunking + embeddings

pgvector

endpoint de búsqueda RAG

Fase 4

sandbox/aislamiento por sesión

workers

métricas/logging

Conclusión técnica

Sí se puede, pero yo cambiaría tu planteamiento así:

No PTY al principio

Sí subprocess normal

No scrapeo de terminal libre

Sí salida estructurada JSON/JSONL

Postgres + pgvector para persistencia y RAG

CLI autenticadas localmente en vez de exponer API keys al front

La versión más sólida de tu idea es:

front simple → backend Rust → ejecuta codex exec o claude -p → parsea JSON/JSONL → guarda en Postgres/pgvector → usa lo persistido como base RAG

Puedo darte ahora mismo un

He vidto que codex al usar codex exec “prompt” tambien te dice cusntos tokens se gastaron y todo eso, me gustaria que esto apareciese en el front de alguna manera para saber cuantos tokens te quedan, cuantos gsdtate hoy, etc (usar la base de datos para hacer bi con qlik)

Buena idea. Si ya vas a persistir todo en Postgres, puedes convertir los datos de codex exec y claude en telemetría de uso y luego explotarlos con Qlik Sense o QlikView. Así puedes ver dashboards de tokens por día, por chat, por modelo, coste estimado, etc. 📊

Te explico cómo estructurarlo para que luego sea fácil de analizar.

1️⃣ Qué datos necesitas capturar

Cuando ejecutes:

codex exec --json "prompt"

el CLI suele emitir eventos JSON donde aparece algo parecido a:

{
  "type": "usage",
  "input_tokens": 120,
  "output_tokens": 450,
  "total_tokens": 570
}

Y al final del run puedes tener también:

{
  "type": "result",
  "cost_usd": 0.0023
}

Claude en modo:

claude -p "prompt" --output-format json

también suele devolver algo similar:

{
  "usage": {
    "input_tokens": 120,
    "output_tokens": 450
  }
}

📌 Lo importante:
no scrapees texto, parsea siempre JSON.

2️⃣ Tabla para métricas de uso

Añade una tabla específica para BI.

CREATE TABLE llm_usage (
    id UUID PRIMARY KEY,
    run_id UUID REFERENCES runs(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    model TEXT,
    input_tokens INT,
    output_tokens INT,
    total_tokens INT,
    estimated_cost_usd NUMERIC,
    created_at TIMESTAMPTZ DEFAULT now()
);

Esto te permite preguntas como:

tokens por usuario

tokens por proveedor

tokens por modelo

coste diario

coste por chat

3️⃣ Relación con las ejecuciones

Relación simple:

chat_sessions
   │
   └── messages
           │
           └── runs
                  │
                  └── llm_usage

Ejemplo real guardado:

provider	model	input_tokens	output_tokens	total_tokens
codex	gpt-5-codex	120	430	550
claude	claude-sonnet	210	800	1010
4️⃣ Backend Rust: parsear tokens

Ejemplo simplificado:

#[derive(serde::Deserialize)]
struct UsageEvent {
    input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    total_tokens: Option<i32>,
}

fn parse_usage(line: &str) -> Option<UsageEvent> {
    serde_json::from_str::<UsageEvent>(line).ok()
}

Cuando detectas el evento:

if let Some(usage) = parse_usage(&line) {
    sqlx::query!(
        r#"
        INSERT INTO llm_usage
        (id, run_id, provider, input_tokens, output_tokens, total_tokens)
        VALUES ($1,$2,$3,$4,$5,$6)
        "#,
        Uuid::new_v4(),
        run_id,
        provider,
        usage.input_tokens,
        usage.output_tokens,
        usage.total_tokens
    )
    .execute(&pool)
    .await?;
}
5️⃣ Mostrar tokens en el frontend

En el chat puedes enseñar algo tipo:

Respuesta generada
──────────────────
Input tokens: 120
Output tokens: 430
Total: 550
Coste estimado: $0.002

o estilo ChatGPT:

550 tokens • Codex
6️⃣ Endpoint de stats

Ejemplo:

GET /stats/tokens/today

Query SQL:

SELECT
    DATE(created_at) as day,
    SUM(input_tokens) as input,
    SUM(output_tokens) as output,
    SUM(total_tokens) as total
FROM llm_usage
GROUP BY day
ORDER BY day DESC;
7️⃣ Dashboard BI con Qlik

En Qlik puedes hacer paneles como:

📊 Tokens diarios
date | tokens
📊 Tokens por proveedor
codex vs claude
📊 Coste estimado
sum(cost_usd)
📊 Top chats que más tokens consumen

join:

llm_usage -> runs -> chat_sessions
8️⃣ Mostrar “tokens restantes”

Esto depende de qué límite tengas.

Opciones:

límite mensual

tabla:

CREATE TABLE usage_limits (
    provider TEXT,
    monthly_limit_tokens INT
);

consulta:

SELECT
limit_tokens - SUM(total_tokens)
FROM llm_usage
límite diario

igual pero por DATE(created_at).

9️⃣ Bonus: tabla agregada para BI

Para que Qlik vuele 🚀 puedes hacer una tabla agregada diaria:

CREATE MATERIALIZED VIEW usage_daily AS
SELECT
    DATE(created_at) as day,
    provider,
    SUM(input_tokens) input_tokens,
    SUM(output_tokens) output_tokens,
    SUM(total_tokens) total_tokens,
    SUM(estimated_cost_usd) cost
FROM llm_usage
GROUP BY day, provider;

Refrescar cada hora.

🔟 UI interesante para tu chat

En cada mensaje podrías mostrar:

Claude Sonnet
820 tokens
$0.0031
2.3s

Y arriba del todo:

Usage today
Tokens: 24,200
Cost: $0.42

💡 Idea potente para tu proyecto

Si guardas todos los eventos JSON de codex/claude, puedes luego hacer:

debugging de prompts

reproducibilidad

análisis de latencia

coste por tipo de prompt

optimización de prompts

Esto convierte tu proyecto en algo parecido a:

LangSmith

Helicone

OpenLLMetry

pero hecho por ti.

✅ Si quieres, también puedo enseñarte una arquitectura muy interesante para esto:

“LLM Gateway local” en Rust

donde tu sistema actúa como:

front
   ↓
rust gateway
   ↓
codex / claude / otros
   ↓
postgres + vector db

y básicamente terminas construyendo tu propio OpenRouter / Helicone local, que es un proyecto muy potente para portfolio.

Mejor que qlik, prefiero conectar mi mcp personal de metabase para que el propio codex pueda generar dashboards; una vez todo este analisis de requisitos hazme un prompt perfecto para que codex me desarrolle un mvp

Buena idea. Si conectas Metabase a tu Postgres y además lo expones mediante tu MCP, entonces Codex puede consultar la base y generar dashboards automáticamente. Eso encaja perfecto con tu arquitectura porque:

todo el uso del LLM queda persistido

Metabase puede leer directamente Postgres

Codex puede crear queries y dashboards desde el propio chat

En otras palabras: tu chat termina siendo auto-observado por el propio LLM 📊🤖

Prompt “perfecto” para que Codex genere el MVP

Te dejo un prompt largo y muy estructurado. Está diseñado para que OpenAI Codex CLI genere directamente el proyecto completo.

Úsalo así:

codex exec "<PROMPT>"

o mejor:

codex exec --json "<PROMPT>"
Escritura

You are a senior Rust systems engineer and AI infrastructure architect.

Your task is to generate a complete MVP project implementing a local LLM orchestration chat system.

The goal is to build a simple chat frontend that executes local CLI AI agents (Codex CLI and Claude CLI) instead of calling APIs directly.

The system must also persist all conversations, runs, and token usage in PostgreSQL, so the data can later be analyzed with Metabase and used for RAG.

Return the result as a complete project structure with code files.

High level architecture

Frontend:

minimal web chat interface

allows:

creating a new chat

sending messages

selecting provider: codex or claude

shows responses

shows token usage per message

Backend:

Rust service

REST API

executes local CLI tools

parses structured output

persists data

Database:

PostgreSQL with pgvector

used for:

chats

messages

LLM runs

token usage

embeddings

future RAG

Analytics:

database schema optimized for BI

compatible with Metabase dashboards

Tech stack

Backend:

Rust

axum

tokio

sqlx

serde

serde_json

uuid

chrono

Database:

PostgreSQL

pgvector extension

Frontend:

simple static HTML + JS

fetch API

minimal CSS

Containerization:

docker-compose

Postgres container with pgvector

Core behavior
Chat flow

User creates chat

User sends prompt

Backend executes one of:

codex exec --json ""

or

claude -p "" --output-format stream-json

Backend captures stdout/stderr

Parse JSON events

Extract:

assistant response

token usage

Persist everything

Return response to frontend

Do NOT scrape text output

Always parse structured JSON from the CLI streams.

Required database schema

chat_sessions

id UUID PK

provider TEXT

title TEXT

created_at TIMESTAMP

messages

id UUID PK

session_id UUID

role TEXT

content TEXT

created_at TIMESTAMP

runs

id UUID PK

session_id UUID

provider TEXT

command TEXT

stdout TEXT

stderr TEXT

raw_events JSONB

status TEXT

created_at TIMESTAMP

finished_at TIMESTAMP

llm_usage

id UUID PK

run_id UUID

provider TEXT

model TEXT

input_tokens INT

output_tokens INT

total_tokens INT

estimated_cost_usd NUMERIC

created_at TIMESTAMP

documents

id UUID PK

session_id UUID

source_run_id UUID

content TEXT

metadata JSONB

chunks

id UUID PK

document_id UUID

chunk_index INT

content TEXT

embedding VECTOR(1536)

Backend endpoints

POST /chat/new

POST /chat/{id}/message

GET /chat/{id}/messages

GET /stats/tokens/today

GET /stats/tokens/total

CLI execution

Use Rust tokio::process::Command

Example:

Command::new("codex")
.arg("exec")
.arg("--json")
.arg(prompt)

or

Command::new("claude")
.arg("-p")
.arg(prompt)
.arg("--output-format")
.arg("stream-json")

Never use shell interpolation.

Token tracking

When JSON usage events appear:

extract

input_tokens
output_tokens
total_tokens

Insert into llm_usage table.

Frontend

Simple interface:

Sidebar:

list of chats

new chat button

Main panel:

messages

input textbox

provider selector

Each assistant message must display:

model
input tokens
output tokens
total tokens

Example:

Response
550 tokens • codex

RAG preparation

After each assistant output:

create document record

chunk text

embeddings placeholder function

store chunks

Actual embedding generation can be stubbed.

Docker setup

Provide docker-compose.yml:

services:

postgres with pgvector

Expose port 5432.

Security constraints

no shell execution

no prompt injection into commands

timeout for CLI runs

max output size

Code quality

modular Rust structure

clear comments

error handling

async safe

Expected output

Generate:

project tree
Cargo.toml
backend source files
frontend files
docker-compose
SQL migrations
README explaining how to run

The project must run locally with:

docker compose up
cargo run

Build a clean MVP suitable for extension into:

RAG system

LLM gateway

analytics platform

Metabase dashboards

Return the full project code.

💡 Tip importante

Cuando ejecutes Codex con este prompt, usa:

codex exec --max-output-tokens 20000

porque va a generar mucho código.

✅ Si quieres, también puedo darte una versión aún mejor del prompt (nivel “agent spec”) que hace que Codex:

escriba el código

cree los tests

cree migraciones SQL

cree dashboards base de Metabase automáticamente.

Investiga en profundidad (dando solo una respuesta 100% correcta de que comandos usara interamente la terminal (para la version de codex que salio hace 1 minuto (buscalo en su web))
Pensó durante 54s

Sí. Mirándolo en la documentación oficial de hoy, 10 de marzo de 2026, la versión más reciente publicada del Codex CLI es 0.113.0.

Para tu MVP, la respuesta correcta es esta:

Comando interno para un chat nuevo

Si quieres ejecutar Codex desde tu backend y leer salida estructurada, el comando que debe lanzar tu terminal interna es:

codex exec --json "PROMPT_DEL_USUARIO"

Eso está documentado oficialmente como el modo no interactivo de Codex, y --json hace que emita eventos JSONL en vez de texto formateado.

Comando interno para continuar un chat existente

Si quieres seguir la misma conversación no interactiva, el comando correcto es:

codex exec resume <SESSION_ID> --json "PROMPT_DEL_USUARIO"

o, si quieres reanudar la última sesión del directorio actual:

codex exec resume --last --json "PROMPT_DEL_USUARIO"

Eso también está documentado oficialmente. codex exec resume [SESSION_ID] continúa una sesión previa y acepta un prompt de seguimiento.

Qué sale por stdout y stderr

En codex exec:

el progreso se envía a stderr

el mensaje final del agente se envía a stdout

con --json, recibes JSONL, un evento por cambio de estado.

La propia documentación muestra un ejemplo real de eventos JSONL como:

thread.started

turn.started

item.started

item.completed

turn.completed

y en turn.completed aparece usage con campos como input_tokens, cached_input_tokens y output_tokens.

Por tanto, los comandos exactos que usaría tu backend

Para una implementación seria de tu frontend/chat con Codex, los comandos internos recomendados quedarían así:

Nuevo chat
codex exec --json "PROMPT_DEL_USUARIO"
Continuar chat existente por session id
codex exec resume <SESSION_ID> --json "PROMPT_DEL_USUARIO"
Continuar la última sesión del directorio actual
codex exec resume --last --json "PROMPT_DEL_USUARIO"
Lo que no puedo afirmar porque OpenAI no lo documenta públicamente

No hay documentación pública que describa “comandos internos más bajos” del CLI, tipo subprocesos privados o llamadas shell internas del propio binario. Lo único que puedo darte como 100% correcto y documentado para la versión actual es el comando público estable que tu terminal debe lanzar: codex exec y, cuando toque, codex exec resume ....

Recomendación final para tu arquitectura

Para tu app en Rust, usaría exactamente esto:

nuevo chat → codex exec --json <prompt>

mismo chat → codex exec resume <session_id> --json <prompt>

parsear los eventos JSONL

guardar session_id, usage, stdout final y eventos crudos en Postgres.

Y un matiz importante: si tú creas “cada new chat = nueva terminal”, eso está bien, pero la continuidad real de conversación en Codex no depende de mantener viva una PTY, sino de reanudar la sesión con codex exec resume. Esa es la forma documentada de continuar una sesión no interactiva.

Si quieres, en el siguiente mensaje te dejo la especificación exacta de backend en Rust con el mapeo new chat / send message / resume, ya aterrizada a tus endpoints.