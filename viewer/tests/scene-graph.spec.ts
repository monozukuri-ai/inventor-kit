import { test, expect } from '@playwright/test';
import { Quaternion, Vector3 } from 'three';
import { identity, renderTree, type Matrix, type Node, type Mesh } from '../src/scene-graph';
import type { Shape, Location } from 'three-cad-viewer';

const mesh = { id: 'shared', triangle_count: 12, bounds: { xmin: 0, xmax: 2, ymin: 0, ymax: 4, zmin: 0, zmax: 6 } } as Mesh;
const shape = {} as Shape;
const values = new Map([['shared', shape]]);
const node = (id: string, local: Matrix | null = identity(), world = local): Node => ({
  id, parent: null, name: 'Same name', kind: 'part', mesh_id: 'shared', status: 'displayable', reason: null,
  local_transform_mm: local, world_transform_mm: world,
});
const apply = (p: Vector3, loc: Location) => p.applyQuaternion(new Quaternion(...loc[1])).add(new Vector3(...loc[0]));

test('nested local placements compose once and repeated meshes retain distinct IDs', () => {
  const a = [[0, -1, 0, 11], [1, 0, 0, 2], [0, 0, 1, 3], [0, 0, 0, 1]];
  const b = [[1, 0, 0, 0], [0, 0, -1, 4], [0, 1, 0, 0], [0, 0, 0, 1]];
  const ab = [[0, 0, 1, 7], [1, 0, 0, 2], [0, 1, 0, 3], [0, 0, 0, 1]];
  const c = identity(); c[0][3] = 30;
  const tree = renderTree([
    { ...node('/document/occ-1', a), kind: 'assembly', mesh_id: null },
    { ...node('/document/occ-1/occ-1', b, ab), parent: '/document/occ-1' }, node('/document/occ-2', c),
  ], [mesh], values);
  const parent = tree.root.parts![0], child = parent.parts![0], other = tree.root.parts![1];
  const point = apply(apply(new Vector3(1, 2, 3), child.loc!), parent.loc!);
  [10, 3, 5].forEach((v, i) => expect(point.toArray()[i]).toBeCloseTo(v, 10));
  expect(child.shape).toBe(other.shape);
  expect(child.id).not.toBe(other.id);
  expect(child.name).toBe('occ-1');
  expect(tree.count).toBe(2);
  expect(tree.triangles).toBe(24);
  expect(tree.root.bb).toEqual({ xmin: 7, xmax: 32, ymin: 0, ymax: 4, zmin: 0, zmax: 7 });
});

test('quaternion conversion preserves half turns around each axis', () => {
  for (let axis = 0; axis < 3; axis++) {
    const m = identity(); for (let i = 0; i < 3; i++) if (i !== axis) m[i][i] = -1;
    const tree = renderTree([node('/document/occ-1', m)], [mesh], values);
    const point = apply(new Vector3(1, 2, 3), tree.root.parts![0].loc!);
    point.toArray().forEach((v, i) => expect(v).toBeCloseTo((i === axis ? 1 : -1) * (i + 1), 10));
  }
});

test('unknown placements remain absent and cannot put a mesh at the origin', () => {
  const unknown = node('/document/occ-1', null);
  expect(() => renderTree([unknown], [mesh], values)).toThrow('unknown placement');
  const tree = renderTree([{ ...unknown, mesh_id: null, status: 'placement_unavailable' }], [], new Map());
  expect(tree.count).toBe(0); expect(tree.root.parts).toEqual([]);
});

test('inconsistent, mirrored and scaled transforms are rejected', () => {
  const moved = identity(); moved[0][3] = 1;
  expect(() => renderTree([node('/document/occ-1', identity(), moved)], [mesh], values)).toThrow('disagree');
  for (const value of [-1, 2]) {
    const m = identity(); m[0][0] = value;
    expect(() => renderTree([node('/document/occ-1', m)], [mesh], values)).toThrow('Unsupported placement');
  }
});
