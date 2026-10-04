import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Router, RouterLink, RouterOutlet } from '@angular/router';
import { finalize } from 'rxjs';
import { UsersApi } from '../users-api';

@Component({
  selector: 'app-layout',
  imports: [RouterLink, RouterOutlet],
  templateUrl: './layout.html',
  styleUrl: './layout.css',
})
export class Layout {
  private readonly api = inject(UsersApi);
  private readonly router = inject(Router);
  private readonly destroyRef = inject(DestroyRef);
  protected readonly loggingOut = signal(false);
  protected readonly error = signal('');

  protected logout() {
    if (this.loggingOut()) return;
    this.loggingOut.set(true);
    this.error.set('');
    this.api.logout().pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.loggingOut.set(false)),
    ).subscribe({
      next: () => this.finishLogout(),
      error: (error: HttpErrorResponse) => {
        if (error.status === 401) this.finishLogout();
        else this.error.set('Could not log out. Please try again.');
      },
    });
  }

  private finishLogout() {
    this.api.clearSession();
    void this.router.navigateByUrl('/login');
  }
}
