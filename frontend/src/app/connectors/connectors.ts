import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Router } from '@angular/router';
import { finalize } from 'rxjs';
import { ProviderConnection, UsersApi } from '../users-api';

@Component({
  selector: 'app-connectors',
  templateUrl: './connectors.html',
  styleUrl: './connectors.css',
})
export class ConnectorsPage {
  private readonly api = inject(UsersApi);
  private readonly router = inject(Router);
  private readonly destroyRef = inject(DestroyRef);
  protected readonly token = signal('');
  protected readonly saving = signal(false);
  protected readonly error = signal('');
  protected readonly connection = signal<ProviderConnection | null>(null);

  protected readonly githubToken = signal('');
  protected readonly githubSaving = signal(false);
  protected readonly githubError = signal('');
  protected readonly githubConnection = signal<ProviderConnection | null>(null);

  protected saveGitlab(event: Event) {
    event.preventDefault();
    if (this.saving()) return;
    const token = this.token().trim();
    this.error.set('');
    if (!token || token.length > 4096 || !/^[\x21-\x7e]+$/.test(token)) {
      this.error.set('Enter a valid token without spaces or line breaks.');
      return;
    }
    this.saving.set(true);
    this.api.authorizeGitlab(token).pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.saving.set(false)),
    ).subscribe({
      next: (connection) => {
        this.connection.set(connection);
        this.token.set('');
      },
      error: (error: HttpErrorResponse) => {
        if (error.status === 401) {
          this.token.set('');
          this.token.set('');
          this.githubToken.set('');
          this.api.clearSession();
          void this.router.navigateByUrl('/login');
        } else {
          this.error.set(error.status === 400
            ? 'GitLab rejected this token. Check that it is active and has read_api and read_repository permissions.'
            : error.status === 503 ? 'GitLab connections are unavailable right now. Please try again later.'
            : 'Could not save your GitLab token. Please try again.');
        }
      },
    });
  }
  protected saveGithub(event: Event) {
    event.preventDefault();
    if (this.githubSaving()) return;
    const token = this.githubToken().trim();
    this.githubError.set('');
    if (!token || token.length > 4096 || !/^[\x21-\x7e]+$/.test(token)) {
      this.githubError.set('Enter a valid token without spaces or line breaks.');
      return;
    }
    this.githubSaving.set(true);
    this.api.authorizeGithub(token).pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.githubSaving.set(false)),
    ).subscribe({
      next: (connection) => {
        this.githubConnection.set(connection);
        this.githubToken.set('');
      },
      error: (error: HttpErrorResponse) => {
        if (error.status === 401) {
          this.githubToken.set('');
          this.token.set('');
          this.githubToken.set('');
          this.api.clearSession();
          void this.router.navigateByUrl('/login');
        } else {
          this.githubError.set(error.status === 400
            ? 'GitHub rejected this token. Check that it is active and has access to your selected repositories.'
            : error.status === 503 ? 'GitHub connections are unavailable right now. Please try again later.'
            : 'Could not save your GitHub token. Please try again.');
        }
      },
    });
  }
}
