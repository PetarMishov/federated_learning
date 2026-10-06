import { Component, inject } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { toSignal } from '@angular/core/rxjs-interop';

@Component({
  selector: 'app-organization',
  templateUrl: './organization.html',
  styleUrl: './organization.css',
})
export class OrganizationPage {
  private readonly route = inject(ActivatedRoute);
  protected readonly queryParams = toSignal(this.route.queryParamMap, {
    initialValue: this.route.snapshot.queryParamMap,
  });
}
