# DegenView Custom — Mejoras pendientes

## Pine Studio

- [ ] Incorporar Forex y TradingView (OpenChart) al selector de instrumentos de Pine Studio, incluyendo los instrumentos Forex de OANDA. Reutilizar los proveedores, el buscador y las credenciales existentes.
- [ ] Al abrir Pine Studio desde un gráfico, heredar automáticamente el activo exacto (incluido proveedor y mercado) y la temporalidad del gráfico activo. Mantener la posibilidad de cambiarlos manualmente dentro del editor.

### Pruebas de aceptación propuestas

- Abrir Pine Studio desde `FOREXCOM:EURUSD` en H1 y comprobar que la vista previa utiliza ese mismo instrumento y H1.
- Repetir con `EUR_USD` de OANDA en M5 y con otro mercado de TradingView.
- Seleccionar otro instrumento o temporalidad manualmente en Pine Studio y verificar que el gráfico principal no cambia por ello.
- Comprobar que los scripts existentes y sus parámetros guardados se conservan.

Estas tareas están pendientes de implementación. No forman parte de las funcionalidades verificadas del DMG actual.
