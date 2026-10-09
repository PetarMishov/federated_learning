import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, ElementRef, HostListener, computed, effect, inject, input, output, signal } from '@angular/core';
import { Router } from '@angular/router';
import { EMPTY, Subscription, catchError, expand, finalize, tap } from 'rxjs';
import { Branch, Repository, RepositoryProvider, UsersApi } from '../users-api';

@Component({
  selector: 'app-branch-picker',
  templateUrl: './branch-picker.html',
  styleUrl: './branch-picker.css',
})
export class BranchPicker {
  readonly provider = input.required<RepositoryProvider>();
  readonly repository = input<Repository | null>(null);
  readonly selectionChange = output<Branch | null>();
  private readonly api = inject(UsersApi);
  private readonly router = inject(Router);
  private readonly element = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly destroyRef = inject(DestroyRef);
  private request: Subscription | undefined;
  private focusTimer: ReturnType<typeof setTimeout> | undefined;
  protected readonly open = signal(false);
  protected readonly loading = signal(false);
  protected readonly error = signal('');
  protected readonly query = signal('');
  protected readonly branches = signal<Branch[]>([]);
  protected readonly selected = signal<Branch | null>(null);
  protected readonly activeIndex = signal(0);
  protected readonly filtered = computed(() => {
    const query = this.query().trim().toLowerCase();
    return this.branches().filter((repo) => repo.name.toLowerCase().includes(query));
  });

  constructor() {
    effect(() => {
      this.provider();
      this.repository();
      this.request?.unsubscribe();
      this.open.set(false);
      this.branches.set([]);
      this.selected.set(null);
      this.query.set('');
      this.error.set('');
      this.activeIndex.set(0);
      this.selectionChange.emit(null);
    });
    this.destroyRef.onDestroy(() => {
      this.request?.unsubscribe();
      clearTimeout(this.focusTimer);
    });
  }

  protected toggle() {
    if (!this.repository()) return;
    if (this.open()) { this.close(); return; }
    this.open.set(true);
    if (!this.branches().length && !this.loading()) this.load();
    this.focusTimer = setTimeout(() => this.element.nativeElement.querySelector<HTMLInputElement>('input')?.focus());
  }

  protected load() {
    this.request?.unsubscribe();
    const repository = this.repository();
    if (!repository) return;
    this.error.set('');
    this.loading.set(true);
    this.branches.set([]);
    const provider = this.provider();
    this.request = this.api.getBranches(provider, repository).pipe(
      expand((page, index) => page.has_more ? this.api.getBranches(provider, repository, index + 2) : EMPTY),
      tap((page) => {
        const merged = new Map(this.branches().map((repo) => [repo.name, repo]));
        for (const repo of page.branches) merged.set(repo.name, repo);
        this.branches.set([...merged.values()].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })));
      }),
      catchError((error: HttpErrorResponse) => {
        if (error.status === 401) {
          this.api.clearSession();
          void this.router.navigateByUrl('/login');
        } else {
          this.error.set(error.status === 404
            ? `Check your ${provider === 'github' ? 'GitHub' : 'GitLab'} connection and repository access.`
            : error.status === 403 ? `Your token cannot read branches for this repository. Check its permissions${provider === 'gitlab' ? ' (read_api is required)' : ' (Contents read access is required)'}.`
            : 'Could not load branches. Please try again.');
        }
        return EMPTY;
      }),
      finalize(() => this.loading.set(false)),
    ).subscribe();
  }

  protected search(value: string) { this.query.set(value); this.activeIndex.set(0); }

  protected choose(branch: Branch) {
    this.selected.set(branch);
    this.selectionChange.emit(branch);
    this.close(true);
  }

  protected dismiss(event: Event) {
    event.preventDefault();
    event.stopPropagation();
    this.close(true);
  }

  protected key(event: KeyboardEvent) {
    if (event.key === 'Escape') { this.dismiss(event); }
    else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const count = this.filtered().length;
      if (!count) return;
      this.activeIndex.update((index) => (index + (event.key === 'ArrowDown' ? 1 : -1) + count) % count);
      this.element.nativeElement.querySelector(`#branch-option-${this.provider()}-${this.activeIndex()}`)?.scrollIntoView?.({ block: 'nearest' });
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const repository = this.filtered()[this.activeIndex()];
      if (repository) this.choose(repository);
    }
  }

  private close(focusTrigger = false) {
    this.open.set(false);
    clearTimeout(this.focusTimer);
    if (focusTrigger) this.element.nativeElement.querySelector<HTMLButtonElement>('.branch-trigger')?.focus();
  }

  @HostListener('document:click', ['$event'])
  protected outsideClick(event: MouseEvent) {
    if (!this.element.nativeElement.contains(event.target as Node)) this.close();
  }

  @HostListener('focusout', ['$event'])
  protected focusOut(event: FocusEvent) {
    if (event.relatedTarget && !this.element.nativeElement.contains(event.relatedTarget as Node)) this.close();
  }
}
