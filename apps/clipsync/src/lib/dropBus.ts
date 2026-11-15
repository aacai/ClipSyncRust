let claimed = false;

export function claimDrops(): void {
  claimed = true;
}

export function releaseDrops(): void {
  claimed = false;
}

export function dropsClaimed(): boolean {
  return claimed;
}
