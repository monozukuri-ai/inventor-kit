import type { Shape, Shapes, Location } from 'three-cad-viewer';

export type Matrix = number[][];
export type BufferRef = { resource: string; dtype: 'float32' | 'uint32'; count: number; bytes: number };
export type Mesh = { id: string; buffers: Record<string, BufferRef>; bounds: Record<string, number>; face_count: number; triangle_count: number };
export type Node = {
  id: string; parent: string | null; name: string; mesh_id: string | null; kind: string; status: string; reason: string | null;
  local_transform_mm: Matrix | null; world_transform_mm: Matrix | null; occurrence_path?: number[];
  definition_key?: string | null; source?: unknown; suppressed?: boolean | null; visible?: boolean | null; substitute?: boolean | null;
};
export const identity = (): Matrix => [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]];
export const multiply = (a: Matrix, b: Matrix): Matrix => a.map(row => b[0].map((_, j) => row.reduce((v, x, k) => v + x * b[k][j], 0)));

function location(m: Matrix): Location {
  // Source matrices are row-major, acting on column vectors in millimetres.
  // The renderer accepts a translation plus an [x, y, z, w] quaternion.
  const trace = m[0][0] + m[1][1] + m[2][2];
  let x: number, y: number, z: number, w: number;
  if (trace > 0) {
    const s = 2 * Math.sqrt(trace + 1);
    w = s / 4; x = (m[2][1] - m[1][2]) / s; y = (m[0][2] - m[2][0]) / s; z = (m[1][0] - m[0][1]) / s;
  } else {
    const i = m[0][0] > m[1][1] && m[0][0] > m[2][2] ? 0 : m[1][1] > m[2][2] ? 1 : 2;
    const j = (i + 1) % 3, k = (i + 2) % 3;
    const s = 2 * Math.sqrt(1 + m[i][i] - m[j][j] - m[k][k]);
    const q = [0, 0, 0]; q[i] = s / 4; q[j] = (m[i][j] + m[j][i]) / s; q[k] = (m[i][k] + m[k][i]) / s;
    [x, y, z] = q; w = (m[k][j] - m[j][k]) / s;
  }
  return [[m[0][3], m[1][3], m[2][3]], [x, y, z, w]];
}

function checked(matrix: Matrix | null): Matrix {
  if (!matrix || matrix.length !== 4 || matrix.some(r => r.length !== 4 || r.some(v => !Number.isFinite(v)))) throw new Error('Unknown or malformed placement');
  if (matrix[3].some((v, i) => Math.abs(v - (i === 3 ? 1 : 0)) > 1e-10)) throw new Error('Unsupported placement');
  const [a, b, c] = matrix;
  const det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
  if (Math.abs(det - 1) > 1e-8) throw new Error('Unsupported placement');
  for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) {
    if (Math.abs(matrix.reduce((n, row, k) => n + (k < 3 ? row[i] * row[j] : 0), 0) - (i === j ? 1 : 0)) > 1e-8) throw new Error('Non-rigid placement');
  }
  return matrix;
}

export function renderTree(nodes: Node[], meshes: Mesh[], values: Map<string, Shape>) {
  const children = new Map<string | null, Node[]>();
  const byId = new Map(nodes.map(n => [n.id, n]));
  if (byId.size !== nodes.length) throw new Error('Duplicate occurrence identity');
  for (const node of nodes) {
    if (node.parent !== null && !byId.has(node.parent)) throw new Error('Missing occurrence parent');
    if (node.id.slice(0, node.id.lastIndexOf('/')) !== (node.parent ?? '/document')) throw new Error('Occurrence path and parent disagree');
    const siblings = children.get(node.parent) ?? []; siblings.push(node); children.set(node.parent, siblings);
  }
  const renderedIds = new Set<string>();
  const visited = new Set<string>();
  let count = 0, triangles = 0;
  const bb: Record<string, number> = { xmin: Infinity, ymin: Infinity, zmin: Infinity, xmax: -Infinity, ymax: -Infinity, zmax: -Infinity };
  const walk = (node: Node, parentWorld: Matrix, depth: number): Shapes | null => {
    if (depth > 64 || visited.has(node.id)) throw new Error('Invalid occurrence hierarchy');
    visited.add(node.id);
    // An unavailable placement is never replaced by an origin placement.
    if (node.local_transform_mm === null || node.world_transform_mm === null) {
      if (node.mesh_id) throw new Error('Display mesh has unknown placement');
      for (const child of children.get(node.id) ?? []) {
        if (walk(child, parentWorld, depth + 1)) throw new Error('Renderable child has unknown parent placement');
      }
      return null;
    }
    const local = node.local_transform_mm;
    const world = multiply(parentWorld, local);
    const parts = (children.get(node.id) ?? []).map(n => walk(n, world, depth + 1)).filter((n): n is Shapes => n !== null);
    if (!node.mesh_id && !parts.length) return null;
    checked(local); checked(node.world_transform_mm);
    if (world.some((row, i) => row.some((v, j) => Math.abs(v - node.world_transform_mm![i][j]) > 1e-8 + 1e-12 * Math.abs(v)))) throw new Error('Local and world placements disagree');
    renderedIds.add(node.id);
    const result: Shapes = { version: 3, id: node.id, name: node.id.split('/').at(-1)!, loc: location(local) };
    if (node.mesh_id) {
      const mesh = meshes.find(m => m.id === node.mesh_id), shape = values.get(node.mesh_id);
      if (!mesh || !shape || parts.length) throw new Error('Invalid occurrence mesh');
      count++; triangles += mesh.triangle_count;
      Object.assign(result, { type: 'shapes', subtype: 'solid', shape, state: [1, 1], color: '#86b5ad', alpha: 1, renderback: false });
      // Bounds only frame the camera; these are not exact CAD measurements.
      for (let corner = 0; corner < 8; corner++) {
        const point = ['x', 'y', 'z'].map((axis, i) => mesh.bounds[axis + (corner & (1 << i) ? 'max' : 'min')]);
        for (let i = 0; i < 3; i++) {
          const v = world[i][3] + point.reduce((sum, p, j) => sum + p * world[i][j], 0);
          bb['xyz'[i] + 'min'] = Math.min(bb['xyz'[i] + 'min'], v); bb['xyz'[i] + 'max'] = Math.max(bb['xyz'[i] + 'max'], v);
        }
      }
    } else result.parts = parts;
    return result;
  };
  const parts = (children.get(null) ?? []).map(n => walk(n, identity(), 0)).filter((n): n is Shapes => n !== null);
  if (visited.size !== nodes.length) throw new Error('Unreachable occurrence');
  const root: Shapes = { version: 3, id: '/document', name: 'document', loc: [[0, 0, 0], [0, 0, 0, 1]], parts, normal_len: 0 };
  if (count) root.bb = bb as unknown as NonNullable<Shapes['bb']>;
  return { root, renderedIds, count, triangles };
}
