import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, OnInit, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { FormsModule } from '@angular/forms';
import { finalize, switchMap } from 'rxjs';
import { Organization, UsersApi } from '../users-api';

@Component({
  selector: 'app-home',
  imports: [FormsModule],
  templateUrl: './home.html',
  styleUrl: './home.css',
})
export class Home implements OnInit {
  protected readonly api = inject(UsersApi);
  private readonly destroyRef = inject(DestroyRef);
  protected readonly organizations = signal<Organization[]>([]);
  protected readonly loading = signal(false);
  protected readonly error = signal('');
  protected username = '';
  protected password = '';

  ngOnInit() {
    if (this.api.token()) this.loadOrganizations();
  }

  protected signIn() {
    if (this.loading() || !this.username.trim() || !this.password) return;
    this.loading.set(true);
    this.error.set('');
    this.api.login(this.username.trim(), this.password).pipe(
      switchMap(() => {
        this.password = '';
        return this.api.getOrganizations();
      }),
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.loading.set(false)),
    ).subscribe({
      next: (result) => this.organizations.set(result.organizations),
      error: (error: HttpErrorResponse) => this.handleError(error),
    });
  }

  protected loadOrganizations() {
    if (this.loading()) return;
    this.loading.set(true);
    this.error.set('');
    this.api.getOrganizations().pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.loading.set(false)),
    ).subscribe({
      next: (result) => this.organizations.set(result.organizations),
      error: (error: HttpErrorResponse) => this.handleError(error),
    });
  }

  private handleError(error: HttpErrorResponse) {
    this.organizations.set([]);
    if (error.status === 401) {
      this.api.clearSession();
      this.password = '';
      this.error.set('Please sign in with a valid username and password. Your session may have expired.');
    } else {
      this.error.set('Could not connect or load organizations. Please try again.');
    }
  }
}
