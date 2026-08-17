// CSS 逻辑视口与 WGPU backing store 的单一同步点。应用总是消费逻辑像素，surface
// 总是消费设备像素，避免 DPR 改变时只重绘左上角或裁切旧帧。

export interface CanvasSurfaceSize {
  readonly logicalWidth: number;
  readonly logicalHeight: number;
  readonly pixelWidth: number;
  readonly pixelHeight: number;
}

/** Calculates logical viewport and updates the backing store for the current device scale. */
export function syncCanvasSurface(canvas: HTMLCanvasElement): CanvasSurfaceSize {
  const bounds = canvas.getBoundingClientRect();
  const ratio = window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
  const pixelWidth = Math.max(1, Math.round(bounds.width * ratio));
  const pixelHeight = Math.max(1, Math.round(bounds.height * ratio));
  if (canvas.width !== pixelWidth) canvas.width = pixelWidth;
  if (canvas.height !== pixelHeight) canvas.height = pixelHeight;
  return {
    logicalWidth: Math.max(320, Math.round(bounds.width)),
    logicalHeight: Math.max(240, Math.round(bounds.height)),
    pixelWidth,
    pixelHeight,
  };
}

/** Observes element size, window size and DPR changes; returns a complete lifecycle cleanup. */
export function observeCanvasSurface(
  canvas: HTMLCanvasElement,
  onChange: (size: CanvasSurfaceSize) => void,
): () => void {
  const sync = () => onChange(syncCanvasSurface(canvas));
  const observer = new ResizeObserver(sync);
  observer.observe(canvas);
  window.addEventListener('resize', sync);

  let resolutionQuery: MediaQueryList | undefined;
  const installResolutionQuery = () => {
    resolutionQuery?.removeEventListener('change', onResolutionChange);
    resolutionQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    resolutionQuery.addEventListener('change', onResolutionChange, { once: true });
  };
  const onResolutionChange = () => {
    sync();
    installResolutionQuery();
  };
  installResolutionQuery();

  return () => {
    observer.disconnect();
    window.removeEventListener('resize', sync);
    resolutionQuery?.removeEventListener('change', onResolutionChange);
  };
}
