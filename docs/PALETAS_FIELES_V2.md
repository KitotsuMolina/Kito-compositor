# Paletas fieles: seis familias y tonos representativos

Implementación local del 28-09-2026, compartida con `refactor/kitsune`.
No se cambian los colores estáticos de la composición de prueba ni se publica una versión.

## Problema resuelto

Antes se reducía la imagen a 96 × 96, se promediaban cubos RGB y se elegían ocho
candidatos por frecuencia. Los acentos pequeños podían perderse. Los roles
claros/medios/oscuros se generaban cambiando HSL de un solo color. Kitsune solo
seleccionaba rojo, verde o azul, y una paleta de archivo podía ocultar el resultado
actual del compositor.

Ahora se recorre la resolución original, contando colores RGB exactos. Las seis
familias tienen sus propios tonos representativos, aunque ocupen muy poco espacio.
Se conservan los campos generales anteriores y el sobre JSON v1; el algoritmo de
paleta se identifica mediante `algorithm_version: 2`.

## Criterio acordado

Se buscan colores reconocibles, no blancos o negros con un matiz mínimo:

- Saturación HSV mínima: 0.35.
- Diferencia RGB máximo − mínimo: al menos 35/255.
- Luminosidad relativa lineal sRGB mínima: 0.025.
- Familias HSV: rojo [330°,360°) y [0°,30°); amarillo [30°,90°); verde [90°,150°);
  cian [150°,210°); azul [210°,270°); magenta [270°,330°).
- Entre candidatos de cada familia: oscuro P10, intermedio P50 y claro P90 de
  luminosidad, ponderados por frecuencia de píxeles. Empates por RGB.

Cada resultado es un RGB existente en la imagen. No se mezclan tonos ni se
fabrican variantes. El intermedio es una mediana ponderada, no la media de los
extremos. Una familia uniforme puede devolver el mismo tono en los tres roles.
Una familia ausente se representa como `null`, nunca como blanco o como otra familia.

Los umbrales son una heurística explícita, no una garantía perceptual universal.
Las familias son amplias: pueden incluir colores cercanos a sus límites. No se
segmentan objetos: el claro y oscuro de una familia pueden proceder de objetos
diferentes. Para una cinta concreta sigue siendo necesario analizar su recorte.
No hay separación física de pigmentos CMY ni análisis espectral.

Los píxeles cuyo alfa no sea 255 se excluyen: no tienen un RGB visible único sin
conocer el fondo bajo ellos. Si no hay píxeles opacos se devuelve un error. Una
imagen gris tiene seis familias ausentes; sus roles generales siguen siendo
muestras reales grises. Se presupone sRGB; no hay gestión ICC explícita.

## Contrato

`palette.families` contiene siempre `red`, `green`, `blue`, `cyan`, `magenta`,
`yellow`. Cada entrada es `null` o:

```json
{
  "light": "#CE3CA9",
  "mid": "#7C2F82",
  "dark": "#552745",
  "pixel_count": 1257,
  "coverage": 0.00062896
}
```

`pixel_count` cuenta todos los candidatos de esa familia, no solamente los píxeles
con los tres HEX elegidos. `coverage` es una fracción respecto a los píxeles opacos
analizados. `sampled_pixels` y `excluded_transparent_pixels` permiten auditarlo.
Los pesos se redondean a ocho decimales para no ocultar acentos pequeños.

Los candidatos generales siguen acotados a ocho, pero primero reservan un tono
intermedio por familia presente; después se completan por frecuencia. Las 18
variantes viajan por `families` y no están limitadas por esos ocho candidatos.
`dominant`, `vibrant`, `muted` y roles generales también proceden de muestras
reales. Negro/blanco para `foreground` es un rol de contraste, no una extracción.

La caché incorpora la versión del algoritmo en su clave y verifica esa versión
al cargar. Conserva ruta, tamaño y modificación del archivo como identidad de
la fuente; no es un hash del contenido. No es necesario borrar cachés antiguos.

## Kitsune

`palette_channel` y sus variantes para partículas/base admiten `r/g/b/c/m/y` y
los nombres ingleses completos. `color_mode=accent_light|accent_mid|accent_dark`
selecciona el tono correspondiente. `static` conserva el color manual.

Ejemplo de opciones de una capa (se incorporan a su objeto completo para
`layers update`; no constituyen por sí solas un objeto de capa válido):

```json
{"color_mode":"accent_mid","palette_channel":"magenta"}
```

Con el contrato v2, `target_luma` se conserva como selector de rol: <0.39 oscuro,
<0.65 intermedio y el resto claro; no genera un RGB interpolado. Conviene omitirlo
si se quiere que el rol lo determine `color_mode`. Los métodos globales
`cmy_c/cmy_m/cmy_y` acompañan a `rgb_r/rgb_g/rgb_b`.

La paleta fiel se aplica directamente: no se altera mediante el suavizado de
colores ni el guard de contraste legacy. Esto preserva los RGB extraídos, pero
puede producir cambios de color perceptibles al cambiar de fondo y no garantiza
contraste en cada punto de la pantalla. Si falta la familia seleccionada se usa
el color de respaldo de la capa; `color resolve` expone esta política y las
familias ausentes. Los contratos antiguos siguen teniendo su ruta legacy.

El compositor pasa a tener prioridad sobre los archivos legacy cuando responde.
Ante fallo, se conserva la última paleta obtenida; si no existe, queda la ruta de
archivo/color manual. `runtime status.palette.outputs` expone imagen, caché,
versión, familias, error y estado obsoleto. El diagnóstico de fuente describe la
consulta al compositor; no identifica cada archivo legacy de respaldo.

En desarrollo, el binario local de compositor junto a los targets tiene prioridad
sobre el instalado; `KITSUNE_COMPOSITOR_BIN` sigue siendo la selección explícita.
La consulta de apariencia tiene límite de 15 s y se ejecuta fuera del hilo gráfico.

## Comandos y validación

```sh
kitsune-compositor appearance preview --image /ruta/fondo.png --no-cache --contract-v1
kitsune-compositor appearance preview --output HDMI-A-1 --contract-v1
kitsune color resolve --output HDMI-A-1
kitsune runtime status
```

Por pantalla se analiza la imagen representativa registrada en active-media,
no una captura de ventanas ni cada fotograma del live wallpaper. Si el registro
está desactualizado, la extracción reflejará esa fuente; el diagnóstico permite verla.

Límites actuales: 32 millones de píxeles y dos millones de colores opacos distintos.
Una entrada superior produce error, no una paleta silenciosamente reducida. Imágenes
grandes pueden tardar más; el resultado queda en caché para las consultas siguientes.

Pruebas incluyen: acentos de un píxel en una imagen de un millón; las seis familias;
exclusión de casi blancos/negros; medianas ponderadas; familias ausentes; grises;
transparencias; invalidación de caché de algoritmo anterior; contrato JSON y
consumo por Kitsune sin generar tonos intermedios ajenos al fondo.

La captura real del usuario recupera, para magenta, `#CE3CA9`, `#7C2F82`, `#552745`.
Evidencia local: `.build-local/palette-v2-capture.json` y
`.build-local/palette-v2-kitsune-resolve.json`, en la raíz del workspace.

### Resultado del cierre

- Compositor: 67 pruebas correctas (50 backend, 2 CLI y 15 contratos).
- Kitsune: 109 pruebas con renderer y 76 headless correctas.
- Clippy sin advertencias en compositor y Kitsune; compilaciones locales correctas.
- Captura original: 1.998.536 píxeles opacos; magenta preservado pese a su baja cobertura.
- Ensayo local de extracción: 1,917 s sin caché y 0,062 s con caché; cifras de esta
  ejecución en modo debug, no una garantía de rendimiento para otras imágenes.
- Ensayo de consumidor: respuesta real del compositor suministrada al CLI mediante
  un proveedor privado de prueba; las seis familias se leen sin crear configuración.
- No se reinició el renderer de la composición activa ni se cambiaron sus colores;
  el nuevo código del renderer se cargará en la próxima instancia. No hay commit,
  release ni actualización de binarios instalados del sistema.
