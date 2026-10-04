import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { finalize } from 'rxjs';
import { UsersApi } from '../users-api';

@Component({
  selector: 'app-login',
  imports: [FormsModule],
  templateUrl: './login.html',
  styleUrl: './login.css',
})
export class Login {
  private readonly api = inject(UsersApi);
  private readonly router = inject(Router);
  private readonly destroyRef = inject(DestroyRef);
  protected username = '';
  protected password = '';
  protected readonly loading = signal(false);
  protected readonly error = signal('');

  protected signIn() {
    if (this.loading() || !this.username.trim() || !this.password) return;
    this.loading.set(true);
    this.error.set('');
    this.api.login(this.username.trim(), this.password).pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.loading.set(false)),
    ).subscribe({
      next: () => {
        this.password = '';
        void this.router.navigateByUrl('/home');
      },
      error: (error: HttpErrorResponse) => {
        this.password = '';
        this.error.set(error.status === 401 ? 'Invalid username or password.' : 'Could not sign in. Please try again.');
      },
    });
  }
}
