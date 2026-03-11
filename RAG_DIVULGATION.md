Informe Técnico: Estrategias de Memoria Externa para LLMs
De los Índices Vectoriales (RAG) a los Grafos de Conocimiento (GraphRAG)

1. El Problema de la Memoria en la IA Generativa
Los Modelos de Lenguaje de Gran Tamaño (LLMs) tienen una limitación crítica: su conocimiento está "congelado" en el tiempo (fecha de corte de entrenamiento) y su "memoria de trabajo" (ventana de contexto) es limitada. Para que RoadToData pueda procesar información propia de forma eficiente, necesitamos estructuras de memoria externa.












2. RAG con Índices Vectoriales (El estándar actual)
Es el sistema que hemos explorado utilizando bases de datos como PostgreSQL con la extensión pgvector.
2.1. Arquitectura Técnica
Fragmentación (Chunking): El documento se divide en trozos de, por ejemplo, 500 palabras.
Embedding: Cada trozo se convierte en un vector (una lista de números que representa su significado semántico).
Recuperación: Cuando el usuario pregunta, buscamos por "Coseno de Similitud" los fragmentos que más se parecen a la pregunta.
2.2. Fortalezas y Debilidades
Ventajas: Implementación rápida, bajo coste computacional y excelente para datos no estructurados masivos.
Limitaciones: Si la respuesta requiere conectar el párrafo 1 del Documento A con el párrafo 20 del Documento B, el sistema suele fallar porque no entiende la relación lógica, solo la proximidad semántica.













3. GraphRAG: La Evolución hacia el Pensamiento Sistémico
GraphRAG (popularizado recientemente por Microsoft Research) introduce una capa intermedia: un Grafo de Conocimiento.
3.1. ¿Cómo funciona el "Indexado de Grafo"?
A diferencia del RAG vectorial, GraphRAG realiza un pre-procesamiento intensivo:
Extracción de Entidades y Relaciones: La IA lee el texto y extrae nodos (Personas, Empresas, Tecnologías) y aristas (Relaciones: "es dueño de", "influye en", "compite con").
Detección de Comunidades: El sistema agrupa nodos relacionados en "comunidades" jerárquicas.
Generación de Resúmenes: Crea resúmenes de cada comunidad de datos antes de que el usuario pregunte nada.
3.2. Diferencias Clave (Para presentar al equipo)
Dimensión
RAG Vectorial (pgvector)
GraphRAG
Unidad de datos
Fragmentos de texto aislados
Entidades y Relaciones interconectadas
Tipo de Consulta
Específica ("¿Cuál es el dato X?")
Global ("¿Qué temas dominan este corpus?")
Razonamiento
Lineal (Encuentra y Lee)
Multi-salto (Conecta puntos A -> B -> C)
Costo
Económico y escalable
Elevado (Requiere muchas llamadas a la API para indexar)


4. Por qué GraphRAG es un buen candidato
Para nuestra división de divulgación, el GraphRAG ofrece capacidades que el RAG simple no puede alcanzar:
Sentido de Contexto: Puede resumir colecciones enteras de documentos sin perderse en los detalles.
Descubrimiento de "Insights": Al ver el grafo, podemos identificar conexiones entre proyectos o clientes que no eran evidentes en los textos sueltos.
Reducción de alucinaciones: Al estar anclado a un grafo de hechos reales extraídos, la IA tiene menos libertad para inventar relaciones que no existen.
5. Próximos Pasos Sugeridos
Prueba de Concepto (PoC): Implementar un motor de GraphRAG sobre nuestra documentación interna de IA.
Hibridación: Explorar sistemas que usen pgvector para búsquedas rápidas y Grafos para razonamiento complejo.








Preparado por: [Francisco Cobos Rodríguez/Gemini 3.1 Pro]
Ubicación: roadtodata/divulgacion/IA

