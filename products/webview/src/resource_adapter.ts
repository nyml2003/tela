// 浏览器资源适配器：平台来源在这里归一化，renderer 只接收 RGBA8。

export interface DecodedImage {
  id: string;
  width: number;
  height: number;
  rgba8: Uint8Array;
}

type ImageSource = string | Blob | ArrayBuffer;

/** 从 URL、data URI、Blob URL 或已取得的字节解码为紧密 RGBA8。 */
export async function decodeImageRgba8(id: string, source: ImageSource): Promise<DecodedImage> {
  const blob = await sourceBlob(source);
  const bitmap = await createImageBitmap(blob);
  try {
    const canvas = document.createElement('canvas');
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (!context) throw new Error('无法创建图片解码 canvas');
    context.clearRect(0, 0, bitmap.width, bitmap.height);
    context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    return {
      id,
      width: bitmap.width,
      height: bitmap.height,
      rgba8: new Uint8Array(pixels),
    };
  } finally {
    bitmap.close();
  }
}

async function sourceBlob(source: ImageSource): Promise<Blob> {
  if (typeof source === 'string') {
    const response = await fetch(source);
    if (!response.ok) throw new Error(`图片请求失败: ${response.status} ${source}`);
    return response.blob();
  }
  if (source instanceof Blob) return source;
  return new Blob([new Uint8Array(source)]);
}
