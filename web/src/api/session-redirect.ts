/** Keeps deliberate sign-out separate from expiry, including requests from an older session. */
export class SessionRedirect {
  private session = Symbol();
  private hasSignOutIntent = false;

  /** Capture the session before I/O; logout suppresses redirects immediately. */
  started(operation: string): symbol {
    if (operation === "logout") this.hasSignOutIntent = true;
    return this.session;
  }

  /** Only a validated sign-in lets new requests redirect for expiry again. */
  succeeded(operation: string): void {
    if (operation === "login" || operation === "bootstrap") {
      this.session = Symbol();
      this.hasSignOutIntent = false;
    }
  }

  /** Failed sign-out leaves the protected page active, so expiry must still work. */
  failed(operation: string): void {
    if (operation === "logout") this.hasSignOutIntent = false;
  }

  /** Old requests cannot displace deliberate navigation or a newer session. */
  permitsExpiry(operation: string, session: symbol): boolean {
    return (
      !this.hasSignOutIntent &&
      session === this.session &&
      operation !== "login" &&
      operation !== "bootstrap"
    );
  }
}
