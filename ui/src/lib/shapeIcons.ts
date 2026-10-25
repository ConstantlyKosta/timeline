// Node shapes are drawn by Sigma's image program as one-colour pictograms: each shape is a tiny
// SVG data URL (black = replaced by the node colour in the shader).
import type { Shape } from './graphModel';

const P: Record<Exclude<Shape, 'circle'>, string> = {
  square: '<rect x="8" y="8" width="84" height="84" rx="6"/>',
  diamond: '<polygon points="50,2 98,50 50,98 2,50"/>',
  triangle: '<polygon points="50,6 96,90 4,90"/>',
  hexagon: '<polygon points="26,6 74,6 98,50 74,94 26,94 2,50"/>',
  star: '<polygon points="50,3 62,36 97,38 69,59 79,93 50,73 21,93 31,59 3,38 38,36"/>',
};
export function shapeIcon(shape: Shape): string {
  const body = shape === 'circle' ? '<circle cx="50" cy="50" r="48"/>' : P[shape];
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100" fill="#000">${body}</svg>`)}`;
}
export function shapeSvg(shape: Shape, color: string, size = 14): string {
  const body = shape === 'circle' ? '<circle cx="50" cy="50" r="46"/>' : P[shape];
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 100 100" fill="${color}">${body}</svg>`;
}
