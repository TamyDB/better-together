// Espelho dos tipos de `dm-protocol` como chegam pelo evento `sim://snapshot` (JSON serde).

export type Animal = "duck" | "capybara" | "horse" | "chicken";
export type Facing = "left" | "right";
export type AnimState = "idle" | "walk" | "run";

export interface Vec2 {
  x: number;
  y: number;
}

export interface PetState {
  id: number;
  animal: Animal;
  pos: Vec2;
  facing: Facing;
  anim: AnimState;
  last_input_seq: number;
}

export interface Snapshot {
  tick: number;
  pets: PetState[];
}
