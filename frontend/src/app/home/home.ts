import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, OnInit, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Router } from '@angular/router';
import { finalize } from 'rxjs';
import { Organization, UsersApi } from '../users-api';

@Component({
  selector: 'app-home',
  templateUrl: './home.html',
  styleUrl: './home.css',
})
export class Home implements OnInit {
  protected readonly api = inject(UsersApi);
  private readonly destroyRef = inject(DestroyRef);
  protected readonly organizations = signal<Organization[]>([]);
  protected readonly loading = signal(false);
  protected readonly error = signal('');
  private readonly router = inject(Router);

  ngOnInit() {
    if (this.api.token()) this.loadOrganizations();
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
      void this.router.navigateByUrl('/login');
    } else {
      this.error.set('Could not connect or load organizations. Please try again.');
    }
  }
}
