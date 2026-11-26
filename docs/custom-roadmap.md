# DegenView Custom — Mejoras pendientes

## Pine Studio

- [ ] Incorporar Forex y TradingView (OpenChart) al selector de instrumentos de Pine Studio, incluyendo los instrumentos Forex de OANDA. Reutilizar los proveedores, el buscador y las credenciales existentes.
- [ ] Al abrir Pine Studio desde un gráfico, heredar automáticamente el activo exacto (incluido proveedor y mercado) y la temporalidad del gráfico activo. Mantener la posibilidad de cambiarlos manualmente dentro del editor.

### Pruebas de aceptación propuestas

- Abrir Pine Studio desde `FOREXCOM:EURUSD` en H1 y comprobar que la vista previa utiliza ese mismo instrumento y H1.
- Repetir con `EUR_USD` de OANDA en M5 y con otro mercado de TradingView.
- Seleccionar otro instrumento o temporalidad manualmente en Pine Studio y verificar que el gráfico principal no cambia por ello.
- Comprobar que los scripts existentes y sus parámetros guardados se conservan.

Las dos tareas ya tienen implementación en la rama de desarrollo; quedan pendientes de validación nativa. El DMG anterior permanece intacto.

## Lectura de indicadores y cruceta

Implementación en desarrollo: leyenda persistente y valores históricos por plot, escala independiente al apuntar a un panel, etiquetas de último valor sin solapamiento y preferencias persistentes del gráfico. La lectura reutiliza VisualOutput; no consulta proveedores al mover el cursor.

### Validación antes de publicar otro DMG

- [ ] `cargo check --locked` y `DEGENVIEW_NO_KEYCHAIN=1 cargo test --locked --lib`.
- [ ] Compilar con OpenChart integrado en macOS ARM64 y comprobar los dos proveedores de Pine Studio.
- [ ] Comparar Williams COT Commercials Net/Index reales con TradingView, incluyendo valores históricos, precisión y cruces de sesión.
- [ ] Comprobar RSI, MACD, overlay, valores positivos/negativos, huecos y offsets; desplazamiento, zoom, replay, paneles maximizados/colapsados, sincronización y capturas.
- [ ] Revisar rendimiento visual y generar DMG solo después de validar.

Las pruebas automatizadas nuevas usan datos sintéticos; no sustituyen la validación de los scripts COT y datos reales.
